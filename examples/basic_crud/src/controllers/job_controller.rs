use gritshield::deps::async_trait;
use gritshield::http::Response;
use gritshield::GritJobExt;
use gritshield::{controller, job, routing::RequestContext, GritJob};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

#[derive(Serialize, Deserialize, GritJob)] // <--- Auto-derives Serialize, Deserialize, and GritJob
pub struct GenerateReportJob {
    pub user_id: String,
    pub format: String,
}
#[derive(Serialize, Deserialize, GritJob)] // <--- Auto-derives Serialize, Deserialize, and GritJob
pub struct GenerateReportJob2 {
    pub user_id: String,
    pub format: String,
}
#[derive(Serialize, Deserialize, GritJob)] // <--- Auto-derives Serialize, Deserialize, and GritJob
pub struct GenerateReportJob3 {
    pub user_id: String,
    pub format: String,
}

#[job(retries = 5)]
impl GenerateReportJob {
    pub async fn perform(&self) -> Result<(), String> {
        println!(
            "Generating report template '{}' to {}",
            self.user_id, self.format
        );
        Ok(())
    }
}
#[job(retries = 5)]
impl GenerateReportJob2 {
    pub async fn perform(&self) -> Result<(), String> {
        println!(
            "Generating report template '{}' to {}",
            self.user_id, self.format
        );
        Ok(())
    }
}
#[job(retries = 5)]
impl GenerateReportJob3 {
    pub async fn perform(&self) -> Result<(), String> {
        println!(
            "Generating report template '{}' to {}",
            self.user_id, self.format
        );
        Ok(())
    }
}

pub struct ReportController;

#[controller("/api/reports")]
impl ReportController {
    #[get("/export")]
    pub async fn export_report(ctx: RequestContext) -> Response {
        let job = GenerateReportJob {
            user_id: "usr_42".to_string(),
            format: "pdf".to_string(),
        };

        // Push job to background worker queue
        let _ = job
            .enqueue_in(&ctx.job_queue, Duration::from_secs(15))
            .await;

        Response::ok("Report generation queued in background!")
    }
}
