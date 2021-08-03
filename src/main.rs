extern crate pretty_env_logger;
#[macro_use]
extern crate log;

use rusoto_codepipeline::{
    CodePipeline, CodePipelineClient, GetPipelineStateInput, ListPipelinesInput, StageExecution,
    StageState,
};
use rusoto_core::credential::ProfileProvider;
use rusoto_core::{HttpClient, Region};

use std::env::{set_var, var};
use std::error::Error;
use std::io;
use tui::backend::CrosstermBackend;
use tui::layout::{Constraint, Direction, Layout};
use tui::style::{Color, Modifier, Style};
use tui::text::Span;
use tui::widgets::BorderType;
use tui::widgets::{Block, Borders};
use tui::Terminal;

#[tokio::main]
// dyn Error: anything that has the Error trait
// Box<dyn Error>: accept anything with the Error trait and put it on the heap instead of the stack
async fn main() -> Result<(), Box<dyn Error>> {
    // RUST_LOG=info would make all our dependencies spit out their logs
    // we don't need to see our imported dependencies' logs, so here we configure our logger to use a custom environment variable instead of RUST_LOG
    set_var("LOCAL_LOGGING", "info");
    pretty_env_logger::try_init_timed_custom_env("LOCAL_LOGGING")?;

    // access credentials through a hardcoded AWS profile named "cdk"
    let credentials_dir = var("HOME")? + "/.aws/credentials";
    let profile_provider = ProfileProvider::with_configuration(credentials_dir, "cdk");
    let http_client = HttpClient::new()?;
    let codepipeline_client =
        CodePipelineClient::new_with(http_client, profile_provider, Region::UsWest2);

    info!("Getting pipelines list...");
    let pipelines_list_res = codepipeline_client
        .list_pipelines(ListPipelinesInput { next_token: None }) // we shouldn't have so many pipelines that a token is necessary
        .await?;
    info!("Successfully listed pipelines.");

    // find the appropriate pipeline by picking the first one with a correct-looking name for now
    let pipelines_list = pipelines_list_res.pipelines.ok_or("No pipelines!")?;
    let dpbuilder_pipeline = pipelines_list
        .into_iter()
        .find(|pipeline| match &pipeline.name {
            Some(name) => name.find("DavidTestStack").is_some(),
            None => false,
        })
        .ok_or_else(|| "Couldn't find the DavidTestStack pipeline!")?;

    let pipeline_name = dpbuilder_pipeline.name.unwrap();
    let get_pipeline_input = GetPipelineStateInput {
        name: pipeline_name.clone(),
    };

    info!("Getting info for pipeline {}...", pipeline_name);
    let dpbuilder_pipeline_info = codepipeline_client
        .get_pipeline_state(get_pipeline_input)
        .await?;
    info!("Successfully got info for pipeline {}.", pipeline_name);

    let stage_states = dpbuilder_pipeline_info.stage_states.unwrap();

    // Make a local clone here so we can inspect and log the states with impunity
    stage_states
        .clone()
        .into_iter()
        .for_each(|elem| match elem {
            StageState {
                latest_execution: Some(execution),
                stage_name: Some(name),
                ..
            } => info!("Stage: {} has status: {}", name, execution.status),
            _ => error!("Could not inspect stage: {:?}", elem),
        });

    let stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    terminal.draw(|f| {
        let titles = ["Stages", "Commits"];
        let sections = titles
            .iter()
            .zip(
                // "zip" to match each title with a Rect
                Layout::default()
                    .direction(Direction::Vertical)
                    .margin(1)
                    .constraints(
                        // generate a constraint for each title
                        // they all have the same constraint in this case (they take up 1/titles.len() of the available space)
                        (0..titles.len())
                            .map(|_| Constraint::Ratio(1, titles.len() as u32))
                            .collect::<Vec<_>>()
                            .as_ref(),
                    )
                    // the available space for this layout is the full area of the terminal `f`
                    .split(f.size()),
            )
            // do an effectful "inspect" here to render each chunk of the layout
            .inspect(|(title, chunk)| {
                f.render_widget(
                    Block::default()
                        .title(Span {
                            content: title.to_string().into(),
                            style: Style::default().add_modifier(Modifier::BOLD),
                        })
                        .border_type(BorderType::Thick)
                        .border_style(Style::default().fg(Color::Rgb(255, 178, 102)))
                        .borders(Borders::ALL),
                    *chunk,
                )
            })
            // we don't need the titles anymore, so discard them
            .map(|(_, chunk)| chunk)
            .collect::<Vec<_>>();

        stage_states
            .iter()
            .zip(
                // each stage will get a Rect
                Layout::default()
                    // fill up the space from left to right
                    .direction(Direction::Horizontal)
                    .margin(1)
                    .constraints(
                        // as above, each Rect will take up a fraction of the space equal to 1/len
                        (0..stage_states.len())
                            .map(|_| Constraint::Ratio(1, stage_states.len() as u32))
                            .collect::<Vec<_>>()
                            .as_ref(),
                    )
                    // the space we're filling up is the first section (the "Stages" chunk) instead of the entire terminal window
                    .split(*sections.get(0).unwrap()),
            )
            // render each stage
            .for_each(|(state, chunk)| {
                f.render_widget(
                    Block::default()
                        .title(Span {
                            content: state.clone().stage_name.unwrap().into(),
                            style: Style::default().add_modifier(Modifier::BOLD),
                        })
                        .border_type(BorderType::Thick)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(
                            match state.to_owned().latest_execution {
                                // if we can get a valid execution state, match on it
                                Some(StageExecution { status, .. }) => match status.as_str() {
                                    "InProgress" => Color::LightBlue,
                                    "Failed" => Color::Red,
                                    "Succeeded" => Color::Green,
                                    _ => Color::LightYellow,
                                },
                                // default to red whenever we can't get the execution state
                                _ => Color::Red,
                            },
                        )),
                    chunk,
                )
            });

        // do the same as above, but this is a structural layout that we'll use for organizing data rather than painting a diagram
        // so no borders/fancy colors are needed
        // also, we're putting it in a different section
        stage_states
            .iter()
            .zip(
                Layout::default()
                    .direction(Direction::Horizontal)
                    .margin(0)
                    .constraints(
                        (0..stage_states.len())
                            .map(|_| Constraint::Ratio(1, stage_states.len() as u32))
                            .collect::<Vec<_>>(),
                    )
                    .split(*sections.get(1).unwrap()),
            )
            .for_each(|(_, chunk)| f.render_widget(Block::default().borders(Borders::NONE), chunk));
    })?;

    Ok(())
}
