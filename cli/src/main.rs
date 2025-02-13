use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use graphviz_rust::cmd::Format;
use process_mining::oc_align::align_case::CaseAlignment;
use process_mining::oc_align::align_case_model::ModelCaseChecker;
use process_mining::oc_align::visualization::case_visual::export_c2_with_alignment_image;
use process_mining::oc_case::dummy_ocel_1_serialization::json_to_case_graph;
use process_mining::oc_case::serialization::deserialize_case_graph;
use process_mining::oc_case::visualization::export_case_graph_image;
use process_mining::oc_petri_net::initialize_ocpn_from_json;
use process_mining::oc_petri_net::marking::Marking;
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use wait_timeout::ChildExt;

/// CLI tool for rigorously testing the branch and bound function on multiple case graphs.
#[derive(Parser)]
#[command(name = "oc_aligner")]
#[command(version = "1.0")]
#[command(author = "Your Name <youremail@example.com>")]
#[command(about = "Processes case graphs with branch and bound", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Controller subcommand to manage and run workers
    Controller {
        /// JSON folder with all case graphs
        #[arg(short, long, value_name = "JSON_FOLDER")]
        json_folder: String,

        /// Path to the petri net JSON file
        #[arg(short, long, value_name = "PETRI_NET")]
        petri_net: String,

        /// Path to the smallest case graph JSON file
        #[arg(short = 's', long, value_name = "SMALLEST_CASE")]
        smallest_case: String,

        /// Output folder
        #[arg(short, long, value_name = "OUTPUT_DIR")]
        output_dir: String,
    },
    /// Worker subcommand to process a single case graph
    Worker {
        /// Path to the petri net JSON file
        #[arg(short, long, value_name = "PETRI_NET")]
        petri_net: String,

        /// Path to the smallest case graph JSON file
        #[arg(short = 's', long, value_name = "SMALLEST_CASE")]
        smallest_case: String,

        /// Path to the case graph JSON file
        #[arg(short, long, value_name = "CASE_GRAPH")]
        case_graph: String,

        /// Output folder
        #[arg(short, long, value_name = "OUTPUT_DIR")]
        output_dir: String,
    },
}

#[derive(Serialize, Deserialize)]
struct CaseStats {
    duration_seconds: f64,
    alignment_cost: f64,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Controller {
            json_folder,
            petri_net,
            smallest_case,
            output_dir,
        } => {
            run_controller(
                Path::new(json_folder),
                Path::new(petri_net),
                Path::new(smallest_case),
                Path::new(output_dir),
            )?;
        }
        Commands::Worker {
            petri_net,
            smallest_case,
            case_graph,
            output_dir,
        } => {
            run_worker(
                Path::new(petri_net),
                Path::new(smallest_case),
                Path::new(case_graph),
                Path::new(output_dir),
            )?;
        }
    }

    Ok(())
}

/// Controller function to manage worker processes
fn run_controller(
    json_folder: &Path,
    petri_net: &Path,
    smallest_case: &Path,
    output_dir: &Path,
) -> Result<()> {
    // Ensure output directory exists
    fs::create_dir_all(output_dir)
        .with_context(|| format!("Failed to create output directory {:?}", output_dir))?;

    // Iterate over case graph files
    let case_graph_files = find_case_graph_files(json_folder)?;

    println!(
        "Found {} case graph files to process.",
        case_graph_files.len()
    );

    // Iterate sequentially
    for case_graph in case_graph_files {
        let file_stem = case_graph
            .file_stem()
            .and_then(OsStr::to_str)
            .ok_or_else(|| anyhow!("Invalid file stem for {:?}", case_graph))?
            .to_owned();

        println!("Processing case graph: {}", case_graph.display());

        // Define log and stats file paths
        let log_path = output_dir.join(format!("{}.log", file_stem));
        let stats_path = output_dir.join(format!("{}.json", file_stem));

        // Prepare the command to run the worker
        let current_exe = std::env::current_exe()?;
        let mut cmd = Command::new(current_exe);
        cmd.arg("worker")
            .arg("--petri-net")
            .arg(petri_net)
            .arg("-s")
            .arg(smallest_case)
            .arg("--case-graph")
            .arg(&case_graph)
            .arg("--output-dir")
            .arg(output_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Spawn the worker process
        let mut child = cmd.spawn().with_context(|| {
            format!(
                "Failed to spawn worker for case graph {}",
                case_graph.display()
            )
        })?;

        let start_time = Instant::now();

        // Set timeout of 2.5 hours
        let timeout = Duration::from_secs(2 * 60 * 60 + 30 * 60); // 2.5 hours

        // Use wait_timeout to wait with timeout
        match child.wait_timeout(timeout)? {
            Some(status) => {
                if status.success() {
                    // Worker succeeded, capture stdout
                    let stdout = child
                        .stdout
                        .take()
                        .ok_or_else(|| anyhow!("Failed to capture stdout"))?;
                    let reader = BufReader::new(stdout);
                    let mut log_file = File::create(&log_path)?;
                    for line in reader.lines() {
                        let line = line?;
                        println!("{}", line);
                        writeln!(log_file, "{}", line)?;
                    }

                    // Capture stderr
                    if let Some(stderr) = child.stderr.take() {
                        let reader = BufReader::new(stderr);
                        for line in reader.lines() {
                            let line = line?;
                            eprintln!("{}", line);
                            writeln!(log_file, "stderr: {}", line)?;
                        }
                    }

                    // Read the stats from stdout
                    // Assuming the worker outputs JSON at the end
                    // Here, since we already wrote all stdout to log, we need another way
                    // A better approach is to have the worker write the stats to separate file
                    // or output to stdout explicitly

                    // For simplicity, let's assume the worker writes the stats to a file
                    let temp_stats_path = output_dir.join(format!("{}_temp.json", file_stem));
                    if temp_stats_path.exists() {
                        let stats_content = fs::read_to_string(&temp_stats_path)?;
                        let stats: CaseStats = serde_json::from_str(&stats_content)?;
                        fs::rename(temp_stats_path, &stats_path)?;
                    } else {
                        return Err(anyhow!(
                            "Worker did not produce stats file for case {}",
                            file_stem
                        ));
                    }

                    let duration = start_time.elapsed();
                    println!(
                        "Completed case {} in {:.2} seconds with cost {}",
                        file_stem,
                        duration.as_secs_f64(),
                        stats_path.to_str().unwrap()
                    );
                } else {
                    eprintln!(
                        "Worker exited with status {:?} for case {}",
                        status,
                        case_graph.display()
                    );
                    // Write to log
                    let mut log_file = File::create(&log_path)?;
                    writeln!(log_file, "Worker exited with status {:?}", status)?;
                    // Write stats with error
                    let stats = CaseStats {
                        duration_seconds: start_time.elapsed().as_secs_f64(),
                        alignment_cost: f64::INFINITY, // Indicate failure
                    };
                    let stats_json = serde_json::to_string(&stats)?;
                    fs::write(&stats_path, stats_json)?;
                }
            }
            None => {
                // Timeout expired, kill the process
                child.kill()?;
                eprintln!(
                    "Worker timed out after {:.2} seconds for case {}",
                    timeout.as_secs_f64(),
                    case_graph.display()
                );
                // Write to log
                let mut log_file = File::create(&log_path)?;
                writeln!(
                    log_file,
                    "Worker timed out after {:.2} seconds",
                    timeout.as_secs_f64()
                )?;
                // Write stats with timeout
                let stats = CaseStats {
                    duration_seconds: timeout.as_secs_f64(),
                    alignment_cost: f64::INFINITY, // Indicate failure
                };
                let stats_json = serde_json::to_string(&stats)?;
                fs::write(&stats_path, stats_json)?;
            }
        }
    }

    Ok(())
}

/// Worker function to process a single case graph
fn run_worker(
    petri_net: &Path,
    smallest_case: &Path,
    case_graph: &Path,
    output_dir: &Path,
) -> Result<()> {
    // Start timing
    let start_time = Instant::now();

    // Initialize ModelCaseChecker
    // Assuming these functions and structs are defined elsewhere in your project
    let json_data =
        fs::read_to_string(petri_net).with_context(|| format!("Reading {:?}", petri_net))?;
    let ocpn = initialize_ocpn_from_json(&json_data);

    let petri_net_arc = std::sync::Arc::new(ocpn);
    let initial_marking = Marking::new(petri_net_arc.clone());

    let shortest_case_json = fs::read_to_string(smallest_case)
        .with_context(|| format!("Reading {:?}", smallest_case))?;
    let shortest_case = deserialize_case_graph(&shortest_case_json);

    let mut checker =
        ModelCaseChecker::new_with_shortest_case(petri_net_arc.clone(), shortest_case);

    // Read the specific case graph
    let case_file = fs::read_to_string(case_graph)
        .with_context(|| format!("Reading case graph {:?}", case_graph))?;
    // Save image of the query case
    // file stem is the name of the input file without extension
    let file_stem = case_graph
        .file_stem()
        .and_then(OsStr::to_str)
        .ok_or_else(|| anyhow!("Invalid file stem for {:?}", case_graph))?;
    let case_graph = json_to_case_graph(&case_file);

    let visualized_dir = output_dir.join("visualized");
    fs::create_dir_all(&visualized_dir)?;

    let query_image_path = visualized_dir.join(format!("{}_query.png", file_stem));
    export_case_graph_image(
        &case_graph,
        query_image_path.to_str().unwrap().to_owned(),
        Format::Png,
        Some(2.0),
    )?;

    // Run branch_and_bound
    let result = checker.branch_and_bound(&case_graph, initial_marking.clone());

    if let Some(result_node) = result {
        println!("Solution found for case {:?}", file_stem);

        // Align and calculate cost
        let alignment = CaseAlignment::align_mip(&case_graph, &result_node.partial_case);
        let cost = alignment.total_cost().unwrap_or(f64::INFINITY);

        // Save aligned image
        let aligned_image_path =
            visualized_dir.join(format!("{}_aligned_cost_{}.png", file_stem, cost));
        export_c2_with_alignment_image(
            &result_node.partial_case,
            &alignment,
            aligned_image_path.to_str().unwrap().to_owned(),
            Format::Png,
            Some(2.0),
        )?;

        // Save target image
        let target_image_path = visualized_dir.join(format!("{}_target.png", file_stem));
        export_case_graph_image(
            &result_node.partial_case,
            target_image_path.to_str().unwrap().to_owned(),
            Format::Png,
            Some(2.0),
        )?;

        // Prepare stats
        let duration = start_time.elapsed().as_secs_f64();
        let stats = CaseStats {
            duration_seconds: duration,
            alignment_cost: cost,
        };

        // Write stats to a temporary file (to be renamed by controller)
        let temp_stats_path = output_dir.join(format!("{}_temp.json", file_stem));
        let stats_json = serde_json::to_string(&stats)?;
        fs::write(&temp_stats_path, stats_json)?;

        println!(
            "Case {} processed in {:.2} seconds with cost {}",
            file_stem, duration, cost
        );
    } else {
        println!("No solution found for case {:?}", file_stem);

        // Prepare stats with infinity cost
        let duration = start_time.elapsed().as_secs_f64();
        let stats = CaseStats {
            duration_seconds: duration,
            alignment_cost: f64::INFINITY,
        };

        // Write stats to a temporary file
        let temp_stats_path = output_dir.join(format!("{}_temp.json", file_stem));
        let stats_json = serde_json::to_string(&stats)?;
        fs::write(&temp_stats_path, stats_json)?;

        println!(
            "Case {} processed in {:.2} seconds with no solution.",
            file_stem, duration
        );
    }

    Ok(())
}

/// Finds all `.jsonocel` files in the given directory
fn find_case_graph_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("Reading directory {:?}", dir))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .and_then(OsStr::to_str)
                .map(|ext| ext.eq_ignore_ascii_case("jsonocel"))
                .unwrap_or(false)
        {
            files.push(path);
        }
    }
    Ok(files)
}
