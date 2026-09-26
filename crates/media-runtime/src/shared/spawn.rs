//! Off-thread DB query helper; the generic spawn helpers live in
//! `big_media_components::player_chrome`.

#[cfg(feature = "audio")]
use big_media_components::player_chrome::spawn_blocking_result;

#[cfg(feature = "audio")]
use crate::db::DbHandle;

/// Run a DB query off-thread and deliver the result on the main loop.
/// Shorthand for `spawn_future_local → spawn_blocking → db.lock()`.
#[cfg(feature = "audio")]
pub fn spawn_db_query<T, Q, U>(db: DbHandle, query: Q, on_result: U)
where
    T: Send + 'static,
    Q: FnOnce(&rusqlite::Connection) -> T + Send + 'static,
    U: FnOnce(T) + 'static,
{
    spawn_blocking_result(
        move || {
            let conn = db.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            query(&conn)
        },
        on_result,
    );
}
