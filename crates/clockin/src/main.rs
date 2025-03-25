use std::collections::BTreeMap;

use chrono::NaiveDate;
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
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    let cli = Command::parse();
    tracing::debug!("Starting cli");

    let dir = dirs::data_dir()
        .expect("to be able to get a data dir")
        .join("clockin")
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
            let days = timetable.group_by_day();
            let days = days.iter().rev().take(limit).collect::<Vec<(_, _)>>();

            for (day, pairs) in days.iter() {
                let hours = pairs
                    .iter()
                    .fold(
                        (chrono::Duration::default(), None),
                        |(total, last_in), ev| match ev.r#type {
                            InOut::In => (total, Some(ev)),
                            InOut::Out => {
                                if let Some(in_time) = last_in {
                                    if in_time.project == project {
                                        (total + (ev.timestamp - in_time.timestamp), None)
                                    } else {
                                        (total, None)
                                    }
                                } else {
                                    (total, None)
                                }
                            }
                            InOut::Break => (total, last_in),
                        },
                    )
                    .0;

                let break_time =
                    pairs
                        .iter()
                        .fold(chrono::TimeDelta::zero(), |acc, e| match e.r#type {
                            InOut::Break => acc + chrono::Duration::minutes(30),
                            _ => acc,
                        });

                println!(
                    "{}: {}h{}m{} mins\n  {}",
                    day,
                    hours.num_hours(),
                    hours.num_minutes() % 60,
                    if break_time.num_minutes() > 0 {
                        format!(", break: {}", break_time.num_minutes())
                    } else {
                        "".into()
                    },
                    pairs
                        .iter()
                        .map(|d| format!(
                            "{} - {}{}",
                            d.timestamp.with_timezone(&chrono::Local).format("%H:%M"),
                            match d.r#type {
                                InOut::In => "clocked in ",
                                InOut::Out => "clocked out",
                                InOut::Break => "break",
                            },
                            if let Some(project) = &d.project {
                                format!(" - project: {}", project)
                            } else {
                                "".into()
                            }
                        ))
                        .collect::<Vec<String>>()
                        .join("\n  ")
                );
            }
        }
        Commands::Break { project } => {
            timetable.days.push(Day {
                timestamp: now,
                r#type: InOut::Break,
                project,
            });
        }
        Commands::In { project } => {
            timetable.days.push(Day {
                timestamp: now,
                r#type: InOut::In,
                project,
            });
        }
        Commands::Out { project } => {
            timetable.days.push(Day {
                timestamp: now,
                r#type: InOut::Out,
                project,
            });
        }
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
    timestamp: chrono::DateTime<chrono::Utc>,
    #[serde(rename = "type")]
    r#type: InOut,

    project: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
enum InOut {
    In,
    Out,
    Break,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
struct TimeTable {
    days: Vec<Day>,
}

impl TimeTable {
    /// Groups entries by calendar day in ascending order by timestamp
    pub fn group_by_day(&self) -> BTreeMap<NaiveDate, Vec<&Day>> {
        let mut grouped: BTreeMap<NaiveDate, Vec<&Day>> = BTreeMap::new();

        // First pass: group entries by date
        for day in &self.days {
            let date = day.timestamp.date_naive();
            grouped.entry(date).or_default().push(day);
        }

        // Second pass: sort each day's entries by timestamp
        for entries in grouped.values_mut() {
            entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        }

        grouped
    }
}
