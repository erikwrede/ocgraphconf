use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use graphviz_rust::cmd::Format;
use crate::oc_case::case::{CaseGraph, Edge, EdgeType, Event, Node, Object};
use crate::oc_case::visualization::export_case_graph_image;

#[derive(Debug, Deserialize)]
struct OcelGlobalLog {
    #[serde(rename = "ocel:attribute-names")]
    attribute_names: Vec<String>,
    #[serde(rename = "ocel:object-types")]
    object_types: Vec<String>,
    #[serde(rename = "ocel:version")]
    version: String,
    #[serde(rename = "ocel:ordering")]
    ordering: String,
}

#[derive(Debug, Deserialize)]
struct OcelGlobalEvent {
    #[serde(rename = "ocel:activity")]
    activity: String,
}

#[derive(Debug, Deserialize)]
struct OcelGlobalObject {
    #[serde(rename = "ocel:type")]
    object_type: String,
}

#[derive(Debug, Deserialize)]
struct OcelEventVMap {
    #[serde(rename = "event_variant")]
    event_variant: Vec<u32>,
    #[serde(rename = "event_id")]
    event_id: u32,
    #[serde(rename = "start_timestamp")]
    start_timestamp: String,
}

#[derive(Debug, Deserialize)]
struct OcelEvent {
    #[serde(rename = "ocel:activity")]
    activity: String,
    #[serde(rename = "ocel:timestamp")]
    timestamp: String,
    #[serde(rename = "ocel:omap")]
    omap: Vec<String>,
    #[serde(rename = "ocel:vmap")]
    vmap: OcelEventVMap,
}

#[derive(Debug, Deserialize)]
struct OcelObjectVMap {
    // Define if there are any object-related attributes
}

#[derive(Debug, Deserialize)]
struct OcelObject {
    #[serde(rename = "ocel:type")]
    object_type: String,
    #[serde(rename = "ocel:ovmap")]
    ovmap: HashMap<String, OcelObjectVMap>,
}

#[derive(Debug, Deserialize)]
struct OcelLog {
    #[serde(rename = "ocel:global-log")]
    global_log: OcelGlobalLog,
    #[serde(rename = "ocel:global-event")]
    global_event: OcelGlobalEvent,
    #[serde(rename = "ocel:global-object")]
    global_object: OcelGlobalObject,
    #[serde(rename = "ocel:events")]
    events: HashMap<String, OcelEvent>,
    #[serde(rename = "ocel:objects")]
    objects: HashMap<String, OcelObject>,
}


pub struct CaseGraphIterator {
    entries: fs::ReadDir,
}

impl CaseGraphIterator {
    pub fn new(source_dir: &str) -> Result<Self, Box<dyn Error>> {
        let source_path = Path::new(source_dir);
        let entries = fs::read_dir(source_path)?;
        Ok(CaseGraphIterator { entries })
    }
}

impl Iterator for CaseGraphIterator {
    type Item = (CaseGraph, PathBuf);

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(entry) = self.entries.next() {
            match entry {
                Ok(entry) => {
                    let path = entry.path();
                    if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("jsonocel") {
                        match fs::read_to_string(&path) {
                            Ok(case_file) => {
                                println!("Processing file: {}", path.display());
                                let case_graph = json_to_case_graph(case_file.as_str());
                                return Some((case_graph, path));
                            }
                            Err(err) => {
                                eprintln!("Failed to read file {}: {}", path.display(), err);
                                continue;
                            }
                        }
                    }
                }
                Err(err) => {
                    eprintln!("Failed to read directory entry: {}", err);
                    continue;
                }
            }
        }
        None
    }
}

pub fn process_jsonocel_files(source_dir: &str) -> Result<(), Box<dyn Error>> {
    let source_path = Path::new(source_dir);
    let visualized_dir = source_path.join("visualized");

    // Create the visualized directory if it doesn't exist
    fs::create_dir_all(&visualized_dir)?;

    // Read the source directory entries
    for entry in fs::read_dir(source_path)? {
        let entry = entry?;
        let path = entry.path();

        // Check if the entry is a file and ends with '.jsonocel'
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("jsonocel") {
            // Load the file as a string
            let case_file = fs::read_to_string(&path)?;

            // Deserialize the case graph from the JSON
            let query_case = json_to_case_graph(case_file.as_str());

            // Construct the path for output image
            let output_file_name = path.file_stem().unwrap().to_str().unwrap().to_owned() + ".png";
            let output_path = visualized_dir.join(output_file_name);

            // Visualize the case graph and export the image
            export_case_graph_image(&query_case, output_path.to_str().unwrap(), Format::Png, Some(0.2))?;
        }
    }

    Ok(())
}

pub fn json_to_case_graph(json_str: &str) -> CaseGraph {
    let ocel = parse_ocel_json(json_str).unwrap();
    ocel_to_case_graph(ocel)
}

// Function to parse OCEL JSON
fn parse_ocel_json(json_str: &str) -> Result<OcelLog, serde_json::Error> {
    serde_json::from_str::<OcelLog>(json_str)
}

// Function to convert OcelLog to CaseGraph
fn ocel_to_case_graph(ocel: OcelLog) -> CaseGraph {
    let mut graph = CaseGraph::new();

    let mut object_id_map: HashMap<String, usize> = HashMap::new();
    let mut event_id_map: HashMap<String, usize> = HashMap::new();

    // Assign unique IDs for objects
    for (obj_key, ocel_object) in &ocel.objects {
        let new_id = graph.get_new_id();
        object_id_map.insert(obj_key.clone(), new_id);

        let object = Object {
            id: new_id,
            object_type: ocel_object.object_type.clone(),
        };
        graph.add_node(Node::ObjectNode(object));
    }

    // Assign unique IDs for events and create Event nodes
    let mut sorted_events: Vec<(&String, &OcelEvent)> = ocel.events.iter().collect();
    // Sort events based on timestamp or event_id for chronological ordering
    sorted_events.sort_by_key(|(_, event)| event.vmap.event_id);

    let mut previous_event_id: Option<usize> = None;

    for (event_key, ocel_event) in sorted_events {
        let new_id = graph.get_new_id();
        event_id_map.insert(event_key.clone(), new_id);

        let event = Event {
            id: new_id,
            event_type: ocel_event.activity.clone(),
        };
        graph.add_node(Node::EventNode(event));

        // If there is a previous event, create a DF (Directly Follows) edge
        if let Some(prev_id) = previous_event_id {
            let edge = Edge::new(graph.get_new_id(), prev_id, new_id, EdgeType::DF);
            graph.add_edge(edge);
        }

        previous_event_id = Some(new_id);

        // Create E2O (Event to Object) edges based on omap
        for oobj in &ocel_event.omap {
            if let Some(&object_id) = object_id_map.get(oobj) {
                let edge = Edge::new(graph.get_new_id(), new_id, object_id, EdgeType::E2O);
                graph.add_edge(edge);
            } else {
                eprintln!("Warning: Object key {} not found in objects map", oobj);
            }
        }
    }

    // Optionally, create O2O (Object to Object) edges if needed
    // This depends on your specific application logic

    graph
}