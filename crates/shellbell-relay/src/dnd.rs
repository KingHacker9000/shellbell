use super::*;

#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct DndStatus {
    pub active: bool,
    pub until: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DndRequest {
    enabled: bool,
    minutes: Option<u32>,
}

pub(super) async fn status(pool: &SqlitePool) -> Result<DndStatus, AppError> {
    let value: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key='dnd'")
        .fetch_optional(pool)
        .await
        .map_err(AppError::db)?;
    let mut status: DndStatus = value
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .map_err(|_| AppError::bad("invalid stored DND state"))?
        .unwrap_or_default();
    if status.until.is_some_and(|until| until <= Utc::now()) {
        status.active = false;
        status.until = None;
    }
    Ok(status)
}

pub(super) async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<DndStatus>, AppError> {
    owner_auth(&state, &headers, false).await?;
    Ok(Json(status(&state.pool).await?))
}

pub(super) async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<DndRequest>,
) -> Result<Json<DndStatus>, AppError> {
    owner_auth(&state, &headers, true).await?;
    if request.minutes.is_some_and(|m| !(1..=10080).contains(&m)) {
        return Err(AppError::bad(
            "DND duration must be between 1 minute and 7 days",
        ));
    }
    let _guard = state.ring_lock.lock().await;
    let status = DndStatus {
        active: request.enabled,
        until: if request.enabled {
            request
                .minutes
                .map(|m| Utc::now() + ChronoDuration::minutes(m.into()))
        } else {
            None
        },
    };
    let mut tx = state.pool.begin().await.map_err(AppError::db)?;
    sqlx::query("INSERT INTO settings(key,value) VALUES('dnd',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value")
        .bind(serde_json::to_string(&status).map_err(|_| AppError::bad("invalid DND state"))?)
        .execute(&mut *tx).await.map_err(AppError::db)?;
    if status.active {
        // Discard pending notification backlog, preserving all history and run state.
        sqlx::query("UPDATE rings SET notifications_muted=1 WHERE notifications_muted=0")
            .execute(&mut *tx)
            .await
            .map_err(AppError::db)?;
    }
    tx.commit().await.map_err(AppError::db)?;
    Ok(Json(status))
}
