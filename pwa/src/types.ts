export type Pairing = { id: string; code: string; display_name: string; created_at: string; expires_at: string }
export type Ring = { event_id: string; source_id: string; source_name: string; message?: string; created_at: string; target_tags: string[] }
export type Source = { id: string; display_name: string; created_at: string; last_seen_at?: string; revoked_at?: string }
export type Receiver = { id: string; name: string; tags: string[]; enabled: boolean; created_at: string }
export type Settings = { history_retention_days: number; vapid_public_key: string }
export type NotificationState = NotificationPermission | 'unsupported'
