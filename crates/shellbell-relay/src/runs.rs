use super::*;

pub(super) fn message(beacon: &RunBeacon) -> String {
    let subject = if let Some(job) = &beacon.job {
        if job.name.is_empty() {
            format!("Slurm job {}", beacon.run_id)
        } else {
            format!("Slurm · {}", job.name)
        }
    } else if beacon.collector == "slurm" {
        format!("Slurm job {}", beacon.run_id)
    } else {
        beacon.collector.clone()
    };
    format!("{} {} on {}", subject, beacon.state.label(), beacon.machine)
        .chars()
        .take(MESSAGE_MAX)
        .collect()
}

/// Periodic cleanup also covers an otherwise idle relay.
pub(super) async fn expire(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let cutoff = Utc::now() - ChronoDuration::days(2);
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM rings WHERE beacon_json IS NOT NULL AND created_at<?")
        .bind(cutoff)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM runs WHERE received_at<?")
        .bind(cutoff)
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}

pub(super) async fn record(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    source: Uuid,
    event: Uuid,
    beacon: &RunBeacon,
    tags: &[String],
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    let json = serde_json::to_string(beacon).map_err(|_| AppError::bad("invalid beacon"))?;
    sqlx::query("UPDATE rings SET beacon_json=? WHERE source_id=? AND event_id=?")
        .bind(&json)
        .bind(source.to_string())
        .bind(event.to_string())
        .execute(&mut **tx)
        .await
        .map_err(AppError::db)?;
    // An older/offline event cannot overwrite newer live status. Heartbeats cannot change state.
    sqlx::query("INSERT INTO runs(source_id,collector,machine,run_id,beacon_json,observed_at,received_at,tags_json) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(source_id,collector,machine,run_id) DO UPDATE SET beacon_json=excluded.beacon_json,observed_at=excluded.observed_at,received_at=excluded.received_at,tags_json=excluded.tags_json WHERE excluded.observed_at>runs.observed_at AND (?=0 OR json_extract(runs.beacon_json,'$.state')=json_extract(excluded.beacon_json,'$.state'))")
        .bind(source.to_string()).bind(&beacon.collector).bind(&beacon.machine).bind(&beacon.run_id)
        .bind(json).bind(beacon.observed_at).bind(now).bind(serde_json::to_string(tags).unwrap()).bind(beacon.heartbeat)
        .execute(&mut **tx).await.map_err(AppError::db)?;
    sqlx::query("DELETE FROM rings WHERE beacon_json IS NOT NULL AND created_at<?")
        .bind(now - ChronoDuration::days(2))
        .execute(&mut **tx)
        .await
        .map_err(AppError::db)?;
    sqlx::query("DELETE FROM runs WHERE received_at<?")
        .bind(now - ChronoDuration::days(2))
        .execute(&mut **tx)
        .await
        .map_err(AppError::db)?;
    Ok(())
}

pub(super) async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    owner_auth(&state, &headers, false).await?;
    snapshot(&state, None).await.map(Json)
}
pub(super) async fn receiver_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let receiver = pull_receiver_auth(&state, &headers).await?;
    if !state.limiter.check(
        format!("receiver-runs:{}", receiver.id),
        60,
        Duration::from_secs(60),
    ) {
        return Err(AppError::rate_limited());
    }
    snapshot(&state, Some(&receiver.tags)).await.map(Json)
}
async fn snapshot(
    state: &AppState,
    receiver_tags: Option<&[String]>,
) -> Result<serde_json::Value, AppError> {
    let rows=sqlx::query("SELECT runs.*,sources.display_name FROM runs JOIN sources ON sources.id=runs.source_id WHERE sources.revoked_at IS NULL AND received_at>? ORDER BY received_at DESC LIMIT 1000")
        .bind(Utc::now()-ChronoDuration::days(2)).fetch_all(&state.pool).await.map_err(AppError::db)?;
    let mut runs = Vec::new();
    for row in rows {
        let tags: Vec<String> = serde_json::from_str(row.get("tags_json")).unwrap_or_default();
        if receiver_tags.is_some_and(|r| !tags_match(r, &tags)) {
            continue;
        }
        let beacon: RunBeacon = serde_json::from_str(row.get("beacon_json"))
            .map_err(|_| AppError::bad("invalid stored beacon"))?;
        let stale = beacon.state.is_active()
            && (Utc::now() - beacon.observed_at).num_seconds()
                > i64::from(beacon.stale_after_seconds);
        runs.push(serde_json::json!({"source_id":row.get::<String,_>("source_id"),"source_name":row.get::<String,_>("display_name"),"beacon":beacon,"stale":stale,"received_at":row.get::<String,_>("received_at")}));
    }
    let mut inbox = Vec::new();
    if receiver_tags.is_none() {
        let rows=sqlx::query("SELECT source_id,event_id,source_name,beacon_json,created_at FROM rings WHERE beacon_json IS NOT NULL AND dismissed=0 AND json_extract(beacon_json,'$.heartbeat')=0 AND created_at>? ORDER BY id DESC LIMIT 200")
            .bind(Utc::now()-ChronoDuration::days(2)).fetch_all(&state.pool).await.map_err(AppError::db)?;
        for row in rows {
            inbox.push(serde_json::json!({"source_id":row.get::<String,_>("source_id"),"event_id":row.get::<String,_>("event_id"),"source_name":row.get::<String,_>("source_name"),"beacon":serde_json::from_str::<serde_json::Value>(row.get("beacon_json")).unwrap_or_default(),"created_at":row.get::<String,_>("created_at")}));
        }
    }
    Ok(serde_json::json!({"runs":runs,"inbox":inbox}))
}
pub(super) async fn clear(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    owner_auth(&state, &headers, true).await?;
    sqlx::query("UPDATE rings SET dismissed=1 WHERE beacon_json IS NOT NULL")
        .execute(&state.pool)
        .await
        .map_err(AppError::db)?;
    Ok(StatusCode::NO_CONTENT)
}
pub(super) async fn dismiss(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((source, event)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    owner_auth(&state, &headers, true).await?;
    sqlx::query(
        "UPDATE rings SET dismissed=1 WHERE source_id=? AND event_id=? AND beacon_json IS NOT NULL",
    )
    .bind(source.to_string())
    .bind(event.to_string())
    .execute(&state.pool)
    .await
    .map_err(AppError::db)?;
    Ok(StatusCode::NO_CONTENT)
}
