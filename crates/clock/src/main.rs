use anyhow::Context;
use chrono::{Local, Timelike, Utc};
use clap::{Parser, Subcommand};
use inquire::validator::Validation;
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
    Resolve {},
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
                    "{}{}{}\n  {}{}\n",
                    day.clock_in.with_timezone(&Local {}).format("%Y-%m-%d"),
                    if let Some(project) = &day.project {
                        format!(" project: {}", project)
                    } else {
                        "".into()
                    },
                    if day.breaks.is_empty() {
                        "".into()
                    } else {
                        format!(
                            " breaks: {}min",
                            day.breaks.iter().fold(0, |acc, _| acc + 30)
                        )
                    },
                    day.clock_in.with_timezone(&Local {}).format("%H:%M"),
                    if let Some(clockout) = &day.clock_out {
                        format!(" - {}", clockout.with_timezone(&Local {}).format("%H:%M"))
                    } else {
                        " - unclosed".into()
                    },
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
        Commands::Resolve {} => {
            let to_resolve = timetable
                .days
                .iter_mut()
                .filter(|d| d.clock_out.is_none())
                .collect::<Vec<_>>();

            if to_resolve.is_empty() {
                println!("Nothing to resolve, good job... :)");
                return Ok(());
            }

            for day in to_resolve {
                let local = day.clock_in.with_timezone(&Local {});
                let clock_in = local.time();
                println!(
                    "Resolve day: {}{}\n  clocked in: {}",
                    day.clock_in.format("%Y/%m/%d"),
                    if let Some(project) = &day.project {
                        format!("\n  project: {}", project)
                    } else {
                        "".into()
                    },
                    day.clock_in.format("%H:%M")
                );

                let output = inquire::Text::new("When did you clock out (16 or 16:30)")
                    .with_validator(move |v: &str| match parse_string_to_time(v) {
                        Ok(time) => {
                            if time <= clock_in {
                                return Ok(Validation::Invalid(
                                    inquire::validator::ErrorMessage::Custom(
                                        "clock out has to be after clockin".into(),
                                    ),
                                ));
                            }

                            Ok(Validation::Valid)
                        }
                        Err(e) => Ok(Validation::Invalid(
                            inquire::validator::ErrorMessage::Custom(e.to_string()),
                        )),
                    })
                    .prompt()?;

                let time = parse_string_to_time(&output)?;
                day.clock_out = Some(
                    local
                        .with_hour(time.hour())
                        .expect("to be able to set hour")
                        .with_minute(time.minute())
                        .expect("to be able to set minute")
                        .with_timezone(&Utc {}),
                );
            }
        }
    }

    if let Some(parent) = dir.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut file = tokio::fs::File::create(dir).await?;
    file.write_all(&serde_json::to_vec_pretty(&timetable)?)
        .await?;
    file.flush().await?;

    Ok(())
}

fn parse_string_to_time(v: &str) -> anyhow::Result<chrono::NaiveTime> {
    chrono::NaiveTime::parse_from_str(v, "%H:%M")
        .or_else(|_| {
            v.parse::<u32>()
                .context("failed to parse to hour")
                .and_then(|h| {
                    if (0..=23).contains(&h) {
                        Ok(h)
                    } else {
                        anyhow::bail!("hours have to be within 0 and 23")
                    }
                })
                .map(|h| chrono::NaiveTime::from_hms_opt(h, 0, 0))
                .ok()
                .flatten()
                .context("failed to parse value")
        })
        .context("failed to parse int to hour")
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
            if d.project != project {
                return false;
            }

            d.clock_in.format("%Y-%m-%d").to_string() == now.format("%Y-%m-%d").to_string()
        });

        item
    }
}
