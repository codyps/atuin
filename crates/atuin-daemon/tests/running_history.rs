//! Running commands must be visible locally before the shell sends its end hook.
#![cfg(unix)]

mod common;

use std::time::Duration;

use atuin_client::settings::Search;
use atuin_daemon::search::SearchIndex;
use common::{TestEnv, history};
use rstest::{fixture, rstest};

#[fixture]
async fn env() -> TestEnv {
    TestEnv::builder().build().await
}

#[rstest]
#[tokio::test]
async fn running_history_is_searchable_and_completed_in_place(#[future(awt)] env: TestEnv) {
    let mut client = env.history_client().await;
    let id =
        client.start_history(history("sleep 12345")).await.unwrap().id.unwrap().try_into().unwrap();
    let pending = env.history_db.load(id).await.unwrap().expect("running row saved");
    assert_eq!((pending.exit, pending.duration), (-1, -1));
    assert!(env.history_records().await.is_empty(), "do not sync unfinished commands");

    let mut search = env.search_client().await;
    assert_eq!(env.rpc_hits(&mut search, "sleep 12345").await, vec![id]);
    // A new index (including one created by a shell-filter change) must see pending rows.
    *env.index.write().await = SearchIndex::default();
    client.rebuild_history().await.unwrap();
    assert_eq!(env.rpc_hits(&mut search, "sleep 12345").await, vec![id]);

    client.end_history(id, Some(Duration::from_secs(12)), 7).await.unwrap();
    let finished = env.history_db.load(id).await.unwrap().unwrap();
    assert_eq!((finished.exit, finished.duration), (7, 12_000_000_000));
    assert_eq!(env.active_rows().await, 1);
    assert_eq!(env.history_records().await.len(), 1);
    assert_eq!(env.rpc_hits(&mut search, "sleep 12345").await, vec![id]);
}

#[rstest]
#[case::cancel(false)]
#[case::delete(true)]
#[tokio::test]
async fn cancelled_running_history_disappears(#[future(awt)] env: TestEnv, #[case] delete: bool) {
    let mut client = env.history_client().await;
    let id =
        client.start_history(history("sleep 12345")).await.unwrap().id.unwrap().try_into().unwrap();
    assert_eq!(env.index_hits("sleep 12345").await, vec![id]);
    if delete {
        client.delete_history(vec![id]).await.unwrap();
    } else {
        env.journal.cancel(id, &Search::default()).await.unwrap();
    }
    assert!(env.history_db.load(id).await.unwrap().is_none());
    assert!(env.index_hits("sleep 12345").await.is_empty());
    assert!(env.history_records().await.is_empty());
    client.rebuild_history().await.unwrap();
    assert!(env.index_hits("sleep 12345").await.is_empty());
    assert!(client.end_history(id, Some(Duration::from_secs(12)), 0).await.is_err());
}

#[rstest]
#[tokio::test]
async fn failed_start_does_not_publish_a_command(#[future(awt)] env: TestEnv) {
    let lock = env.lock_history_db().await;
    let entry = history("sleep 12345");
    let id = entry.id;
    assert!(env.journal.start_cmd(entry).await.is_err());
    assert!(env.journal.get(id).is_err());
    assert!(env.index_hits("sleep 12345").await.is_empty());
    assert!(env.history_records().await.is_empty());
    lock.release().await;
    assert!(env.history_db.load(id).await.unwrap().is_none());
}

#[rstest]
#[case::cancel(false)]
#[case::delete(true)]
#[tokio::test]
async fn failed_cancellation_keeps_running_history(
    #[future(awt)] env: TestEnv,
    #[case] delete: bool,
) {
    let id = env.journal.start_cmd(history("sleep 12345")).await.unwrap();
    let lock = env.lock_history_db().await;
    if delete {
        assert!(env.journal.delete(&[id], &Search::default()).await.is_err());
    } else {
        assert!(env.journal.cancel(id, &Search::default()).await.is_err());
    }
    assert!(env.journal.get(id).is_ok());
    assert!(env.history_db.load(id).await.unwrap().is_some());
    assert_eq!(env.index_hits("sleep 12345").await, vec![id]);
    lock.release().await;
    env.journal.cancel(id, &Search::default()).await.unwrap();
    assert!(env.history_db.load(id).await.unwrap().is_none());
    assert!(env.index_hits("sleep 12345").await.is_empty());
}
