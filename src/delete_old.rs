use tokio_cron_scheduler::{Job, JobScheduler, JobSchedulerError};
use std::time::SystemTime;
use mongodb::{bson::doc, Database};

use crate::types::POI;

/// returns # of days since the epoch
pub fn today() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap()
                    .as_secs() / 60 / 60 / 24
}

pub async fn begin(db: Database) -> Result<(), JobSchedulerError> {

    let sched = JobScheduler::new().await?;

    
    // Add async job
    sched.add(
        Job::new_async("* * * * * *", move |_, _| {

            let db = db.clone();

            Box::pin(async move {

                let res = db.collection::<POI>("poi")
                    .delete_many(doc! { "expiry": {"$lt": today() as i32} }).await;
                
                println!("delete old posts execution result: {:?}", res);
            })
        })?
    ).await?;

    sched.start().await?;

    Ok(())
}