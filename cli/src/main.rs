use clap::{ArgGroup, Parser, Subcommand};
use graphviz_rust::cmd::Format;
use log::{error, info, LevelFilter};
use process_mining::oc_align::align_case::CaseAlignment;
use process_mining::oc_align::align_case_model::ModelCaseChecker;
use process_mining::oc_align::visualization::case_visual::export_c2_with_alignment_image;
use process_mining::oc_case::dummy_ocel_1_serialization::CaseGraphIterator;
use process_mining::oc_case::serialization::deserialize_case_graph;
use process_mining::oc_case::visualization::export_case_graph_image;
use process_mining::oc_petri_net::initialize_ocpn_from_json;
use process_mining::oc_petri_net::marking::Marking;
use serde::Deserialize;
use simplelog::{Config as LogConfig, WriteLogger};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Instant,
};
use std::sync::Arc;

/// Command-line arguments for the application
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(group(
    ArgGroup::new("mode")
        .required(true)
        .args(&["controller", "worker"]),
))]
struct Args {
    /// Run in controller mode
    #[arg(short, long, group = "mode")]
    controller: bool,

    /// Run in worker mode
    #[arg(short, long, group = "mode")]
    worker: bool,

    /// Input directory containing the Petri net and case graphs
    #[arg(short, long, required(false))] // Required only for controller
    input_dir: Option<String>,

    /// Output directory for logs and results
    #[arg(short, long, required(false))] // Required only for controller
    output_dir: Option<String>,

    /// Path to the Petri net JSON file relative to input_dir
    #[arg(short = 'p', long, default_value = "oc_petri_net.json")]
    petri_net_file: String,

    /// Path to the shortest case JSON file relative to input_dir
    #[arg(short = 's', long, default_value = "shortest_case_graph.json")]
    shortest_case_file: String,

    /// Path to the case graphs directory relative to input_dir
    #[arg(short = 'c', long, default_value = "problemkinder/crash")]
    case_graph_dir: String,

    /// Case name to process (only for worker)
    #[arg(short = 'n', long, required(false))] // Required only for worker
    case_name: Option<String>,

    /// Path to the specific case file to process (only for worker)
    #[arg(short = 'f', long, required(false))] // Required only for worker
    case_file: Option<String>,
}

fn main() {
    let args = Args::parse();

    if args.controller {
        run_controller(args);
    } else if args.worker {
        run_worker(args);
    }
}

/// Controller Functionality
fn run_controller(args: Args) {
    // Validate required arguments
    let input_dir = match args.input_dir {
        Some(dir) => Path::new(&dir).to_owned(),
        None => {
            eprintln!("Controller mode requires --input_dir");
            std::process::exit(1);
        }
    };

    let output_dir = match args.output_dir {
        Some(dir) => PathBuf::from(dir),
        None => {
            eprintln!("Controller mode requires --output_dir");
            std::process::exit(1);
        }
    };

    // Initialize global logging
    let log_file = output_dir.join("controller.log");
    if let Err(e) = fs::create_dir_all(&output_dir) {
        eprintln!("Failed to create output directory {:?}: {}", output_dir, e);
        std::process::exit(1);
    }

    if let Err(e) = WriteLogger::init(
        LevelFilter::Info,
        LogConfig::default(),
        fs::File::create(&log_file).unwrap(),
    ) {
        eprintln!("Failed to initialize logger: {}", e);
        std::process::exit(1);
    }

    info!(
        "Starting controller with input_dir: {:?}, output_dir: {:?}",
        input_dir, output_dir
    );

    // Initialize CaseGraphIterator
    let case_graph_dir = input_dir.join(&args.case_graph_dir);
    let case_graph_iter = match CaseGraphIterator::new(&case_graph_dir.to_str().unwrap()) {
        Ok(iter) => iter,
        Err(e) => {
            error!("Failed to initialize CaseGraphIterator: {}", e);
            std::process::exit(1);
        }
    };

    // Prepare visualized directory (Ensure it's created for workers)
    let visualized_dir = output_dir.join("varsbpi_visualized");
    if let Err(e) = fs::create_dir_all(&visualized_dir) {
        error!(
            "Failed to create visualized directory {:?}: {}",
            visualized_dir, e
        );
        std::process::exit(1);
    }

    // Iterate over each case and spawn a worker process
    for (case_graph, path) in case_graph_iter {
        let case_name = match path.file_stem().and_then(|s| s.to_str()) {
            Some(name) => name.to_owned(),
            None => {
                error!("Invalid case file name: {:?}", path);
                continue;
            }
        };

        info!("Spawning worker for case: {}", case_name);

        // Prepare paths
        let case_file = match path.to_str() {
            Some(s) => s.to_owned(),
            None => {
                error!("Failed to convert case file path to string: {:?}", path);
                continue;
            }
        };

        let worker_executable = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(e) => {
                error!("Failed to get current executable path: {}", e);
                continue;
            }
        };

        // Spawn the worker process
        let status = Command::new(worker_executable)
            .arg("--worker")
            .arg("--input_dir")
            .arg(input_dir.to_str().unwrap())
            .arg("--output_dir")
            .arg(output_dir.to_str().unwrap())
            .arg("--petri_net_file")
            .arg(&args.petri_net_file)
            .arg("--shortest_case_file")
            .arg(&args.shortest_case_file)
            .arg("--case_graph_dir")
            .arg(&args.case_graph_dir)
            .arg("--case_name")
            .arg(&case_name)
            .arg("--case_file")
            .arg(&case_file)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .status();

        match status {
            Ok(status) => {
                if status.success() {
                    info!("Worker succeeded for case: {}", case_name);
                } else {
                    error!(
                        "Worker failed for case: {} with status: {}",
                        case_name, status
                    );
                }
            }
            Err(e) => {
                error!("Failed to spawn worker for case {}: {}", case_name, e);
            }
        }
    }

    info!("All cases have been dispatched to workers.");
}

/// Worker Functionality
fn run_worker(args: Args) {
    // Validate required arguments
    let input_dir = match args.input_dir {
        Some(dir) => Path::new(&dir).to_owned(),
        None => {
            eprintln!("Worker mode requires --input_dir");
            std::process::exit(1);
        }
    };

    let output_dir = match args.output_dir {
        Some(dir) => PathBuf::from(dir),
        None => {
            eprintln!("Worker mode requires --output_dir");
            std::process::exit(1);
        }
    };

    let case_name = match args.case_name {
        Some(name) => name,
        None => {
            eprintln!("Worker mode requires --case_name");
            std::process::exit(1);
        }
    };

    let case_file = match args.case_file {
        Some(file) => file,
        None => {
            eprintln!("Worker mode requires --case_file");
            std::process::exit(1);
        }
    };

    // Each worker initializes its own logger
    let visualized_dir = output_dir.join("varsbpi_visualized");
    if let Err(e) = fs::create_dir_all(&visualized_dir) {
        eprintln!(
            "Failed to create visualized directory {:?}: {}",
            visualized_dir, e
        );
        std::process::exit(1);
    }
    let case_log_file = visualized_dir.join(format!("{}.log", case_name));

    if let Err(e) = WriteLogger::init(
        LevelFilter::Info,
        LogConfig::default(),
        fs::File::create(&case_log_file).unwrap(),
    ) {
        eprintln!("Failed to initialize logger for case {}: {}", case_name, e);
        std::process::exit(1);
    }

    info!("Starting worker for case: {}", case_name);

    // Read and initialize Petri net
    let petri_net_path = input_dir.join(&args.petri_net_file);
    let json_data = match fs::read_to_string(&petri_net_path) {
        Ok(data) => data,
        Err(e) => {
            error!("Failed to read Petri net file {:?}: {}", petri_net_path, e);
            std::process::exit(1);
        }
    };
    let ocpn =  initialize_ocpn_from_json(&json_data);
    let initial_marking = Marking::new(Arc::new(ocpn.clone()));

    // Deserialize the shortest case
    let shortest_case_path = input_dir.join(&args.shortest_case_file);
    let shortest_case_json = match fs::read_to_string(&shortest_case_path) {
        Ok(data) => data,
        Err(e) => {
            error!(
                "Failed to read shortest case file {:?}: {}",
                shortest_case_path, e
            );
            std::process::exit(1);
        }
    };
    let shortest_case =  deserialize_case_graph(&shortest_case_json);

    // Initialize ModelCaseChecker
    let petri_net_arc = Arc::new(ocpn);
    let mut checker =
        ModelCaseChecker::new_with_shortest_case(petri_net_arc.clone(), shortest_case);

    // Load the specific case graph
    let case_graph_json = match fs::read_to_string(&case_file) {
        Ok(data) => data,
        Err(e) => {
            error!("Failed to read case file {:?}: {}", case_file, e);
            std::process::exit(1);
        }
    };
    let case_graph =  deserialize_case_graph(&case_graph_json);

    // Start timing
    let start_time = Instant::now();

    // Prepare output paths
    let output_file_base = visualized_dir.join(&case_name);

    // Process the case
    // Removed `std::panic::catch_unwind` as all initialization and processing are within the worker
    // Any panic here will terminate the worker process without affecting the controller
    // This ensures the controller remains stable regardless of worker failures
    {
        // Export the query case image
        if let Err(e) = export_case_graph_image(
            &case_graph,
            &(output_file_base.to_str().unwrap().to_owned() + "_query.png"),
            Format::Png,
            Some(2.0),
        ) {
            error!("Failed to export query case image: {}", e);
            std::process::exit(1);
        }

        // Run branch and bound
        let branch_result = checker.branch_and_bound(&case_graph, initial_marking.clone());

        if let Some(result_node) = branch_result {
            info!("Solution found for case {}", case_name);

            // Align the case
            let alignment = CaseAlignment::align_mip(&case_graph, &result_node.partial_case);
            let cost = alignment.total_cost().unwrap_or(f64::INFINITY);

            // Export alignment images
            if let Err(e) = export_c2_with_alignment_image(
                &result_node.partial_case,
                &alignment,
                &(output_file_base.to_str().unwrap().to_owned()
                    + &format!("_aligned_cost_{}.png", cost)),
                Format::Png,
                Some(2.0),
            ) {
                error!("Failed to export alignment image: {}", e);
                std::process::exit(1);
            }

            if let Err(e) = export_case_graph_image(
                &result_node.partial_case,
                &(output_file_base.to_str().unwrap().to_owned() + "_target.png"),
                Format::Png,
                Some(2.0),
            ) {
                error!("Failed to export target case graph image: {}", e);
                std::process::exit(1);
            }

            // Log alignment details
            info!(
                "Case: {}\nAlignment Cost: {}\nAlignment Details: {:?}",
                case_name, cost, alignment
            );
        } else {
            info!("No solution found for case {}", case_name);
            info!("Case: {}\nNo solution found.", case_name);
        }
    }

    // Stop timing and log duration
    let duration = start_time.elapsed();
    info!(
        "Finished processing case: {} in {:.2?}",
        case_name, duration
    );
    info!(
        "Case: {}\nProcessing Time: {:.2?}",
        case_name, duration
    );

    // Exit with success
    std::process::exit(0);
}