use chrono::Timelike;
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

#[derive(Parser)]
#[command(author, version, about, long_about = None, subcommand_required = true)]
struct Command {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    In {
        #[arg(long = "project")]
        project: Option<String>,
    },
    Out {
        #[arg(long = "project")]
        project: Option<String>,
    },
    Break {
        #[arg(long = "project")]
        project: Option<String>,
    },
    List {
        #[arg(long = "limit", default_value = "5")]
        limit: usize,

        #[arg(long = "project")]
        project: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt::init();

    let cli = Command::parse();
    tracing::debug!("Starting cli");

    let dir = dirs::data_dir()
        .expect("to be able to get a data dir")
        .join("clock")
        .join("timetable.json");

    let mut timetable = if dir.exists() {
        let timetable = tokio::fs::read(&dir).await?;
        let timetable: TimeTable = serde_json::from_slice(&timetable)?;
        timetable
    } else {
        TimeTable::default()
    };

    let now = chrono::Utc::now();

    match cli.command.expect("to have a command available") {
        Commands::List { limit, project } => {
            let days = &timetable
                .days
                .iter()
                .filter(|d| {
                    if let Some(project) = &project {
                        Some(project) == d.project.as_ref()
                    } else {
                        true
                    }
                })
                .collect::<Vec<_>>();
            let days = days.iter().rev().take(limit).collect::<Vec<_>>();

            for day in days {
                println!(
                    "day: {}{}\n  {}:{}{}",
                    day.clock_in.format("%Y/%m/%d"),
                    if let Some(project) = &day.project {
                        format!(" project: {}", project)
                    } else {
                        "".into()
                    },
                    day.clock_in.hour(),
                    day.clock_in.minute(),
                    if let Some(clockout) = &day.clock_out {
                        format!(" - {}:{}", clockout.hour(), clockout.minute())
                    } else {
                        " - unclosed".into()
                    }
                )
            }
        }
        Commands::In { project } => {
            timetable.days.push(Day {
                clock_in: now,
                clock_out: None,
                breaks: Vec::default(),
                project,
            });
        }
        Commands::Out { project } => match timetable.get_day(project, now) {
            Some(day) => day.clock_out = Some(now),
            None => todo!(),
        },
        Commands::Break { project } => match timetable.get_day(project, now) {
            Some(day) => day.breaks.push(Break {}),
            None => todo!(),
        },
    }

    if let Some(parent) = dir.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut file = tokio::fs::File::create(dir).await?;
    file.write_all(&serde_json::to_vec(&timetable)?).await?;
    file.flush().await?;

    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Day {
    clock_in: chrono::DateTime<chrono::Utc>,
    clock_out: Option<chrono::DateTime<chrono::Utc>>,

    breaks: Vec<Break>,

    project: Option<String>,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
struct Break {}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
struct TimeTable {
    days: Vec<Day>,
}

impl TimeTable {
    pub fn get_day(
        &mut self,
        project: Option<String>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Option<&mut Day> {
        let item = self.days.iter_mut().find(|d| {
            if d.project == project {
                return false;
            }

            d.clock_in.format("%Y-%m-%d").to_string() == now.format("%Y-%m-%d").to_string()
        });

        item
    }
}
