use tokio_cron_scheduler::{Job, JobScheduler, JobSchedulerError};
use std::time::SystemTime;
use mongodb::{bson::{doc, Document}, results::DeleteResult, Database};

/// returns # of days since the epoch
pub fn today() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap()
        .as_secs() / 60 / 60 / 24
}

async fn clear_old_posts(db: Database) -> Result<DeleteResult, mongodb::error::Error> {
    db.collection::<Document>("posts")
        .delete_many(doc! { "expiry": {"$lt": today() as i32} })
        .await
}

pub async fn start_task(db: Database) -> Result<(), JobSchedulerError> {

    println!("starting clear old post task", );

    clear_old_posts(db.clone()).await.unwrap();

    let sched = JobScheduler::new().await?;

    sched.add(
        // run every day at 00:00
        Job::new_async("0 0 0 * * *", move |_, _| {

            let db = db.clone();

            Box::pin(async move {
                println!(
                    "clear old posts result: {:?}", 
                    clear_old_posts(db).await
                );
            })
        })?
    ).await?;

    sched.start().await?;

    Ok(())
}