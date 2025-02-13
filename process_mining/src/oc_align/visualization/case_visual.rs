use crate::oc_align::align_case::{CaseAlignment, EdgeMapping, NodeMapping};
use crate::oc_case::case::Node::{EventNode, ObjectNode};
use crate::oc_case::case::{
    CaseGraph, Edge as CaseEdge, EdgeType,
};
use graphviz_rust::{
    cmd::Format,
    dot_generator::{attr, edge, graph, id, node, node_id, stmt},
    dot_structures::*,
    printer::{DotPrinter, PrinterContext},
};
use std::collections::HashSet;
use std::{fs::File, io::Write};
use uuid::Uuid;

/// Export the image of `c2` with alignment highlighting.
///
/// - Original `c2` nodes and edges:
///     - Mapped: Black
///     - Unmapped: Red
/// - Void nodes and edges:
///     - Blue
///
/// Only void nodes and edges that are used in the alignment are added.
///
/// # Arguments
///
/// * `c2` - The target `CaseGraph` to visualize.
/// * `alignment` - The `CaseAlignment` resulting from mapping `c1` to `c2`.
/// * `path` - The file path to save the image.
/// * `format` - The image format (e.g., PNG, PDF).
/// * `dpi_factor` - Optional DPI scaling factor.
///
/// # Returns
///
/// * `Result<(), std::io::Error>` - Ok if successful, Err otherwise.
pub fn export_c2_with_alignment_image<P: AsRef<std::path::Path>>(
    c2: &CaseGraph,
    alignment: &CaseAlignment,
    path: P,
    format: Format,
    dpi_factor: Option<f32>,
) -> Result<(), std::io::Error> {
    let g = export_c2_with_alignment_to_dot_graph(c2, alignment, dpi_factor);
    let dot_string = g.print(&mut PrinterContext::default());
    let out = graphviz_rust::exec(g, &mut PrinterContext::default(), vec![format.into()])?;
    let mut f = File::create(path)?;
    f.write_all(&out)?;
    Ok(())
}

/// Export `c2` along with alignment to a DOT graph.
///
/// Original `c2` nodes and edges are colored based on their mapping status.
/// Void nodes and edges are added in blue if they are used in the alignment.
///
/// # Arguments
///
/// * `c2` - The target `CaseGraph` to visualize.
/// * `alignment` - The `CaseAlignment` containing mapping information.
/// * `dpi_factor` - Optional DPI scaling factor.
///
/// # Returns
///
/// * `Graph` - The DOT graph structure.
pub fn export_c2_with_alignment_to_dot_graph(
    c2: &CaseGraph,
    alignment: &CaseAlignment,
    dpi_factor: Option<f32>,
) -> Graph {
    // Step 1: Identify mapped c2 node IDs
    let mapped_c2_node_ids: HashSet<usize> = alignment
        .node_mapping
        .values()
        .filter_map(|mapping| {
            if let NodeMapping::RealNode(_, c2_id) = mapping {
                Some(*c2_id)
            } else {
                None
            }
        })
        .collect();

    // Step 2: Identify mapped c2 edge IDs
    let mapped_c2_edge_ids: HashSet<usize> = alignment
        .edge_mapping
        .values()
        .filter_map(|mapping| {
            if let EdgeMapping::RealEdge(_, c2_id) = mapping {
                Some(*c2_id)
            } else {
                None
            }
        })
        .collect();

    // Step 3: Prepare node statements with color coding
    let mut node_stmts: Vec<Stmt> = Vec::new();

    for node in c2.nodes.values() {
        let color = if mapped_c2_node_ids.contains(&node.id()) {
            "black"
        } else {
            "red" // Unmapped nodes
        };
        let label = match node {
            EventNode(event) => event.event_type.clone(),
            ObjectNode(object) => object.object_type.clone(),
        };
        let shape = match node {
            EventNode(_) => "ellipse",
            ObjectNode(_) => "box",
        };
        node_stmts.push(stmt!(node!(
            esc format!("{}", node.id());
            attr!("label", esc &label),
            attr!("shape", shape),
            attr!("color", color),
            attr!("fontcolor", color)
        )));
    }

    // Step 4: Prepare edge statements with color coding
    let mut edge_stmts: Vec<Stmt> = Vec::new();

    for edge in c2.edges.values() {
        let color = if mapped_c2_edge_ids.contains(&edge.id) {
            "black"
        } else {
            "red" // Unmapped edges
        };
        let edge_label = match edge.edge_type {
            EdgeType::DF => "DF",
            EdgeType::O2O => "O2O",
            EdgeType::E2O => "E2O",
        };
        edge_stmts.push(stmt!(edge!(
            node_id!(esc format!("{}", edge.from)) => node_id!(esc format!("{}", edge.to));
            attr!("label", esc edge_label),
            attr!("color", color)
        )));
    }

    // Step 5: Add void nodes (in blue) if they exist
    for (&void_id, void_node) in &alignment.void_nodes {
        let label = match void_node {
            EventNode(event) => format!("Void Event: {}", event.event_type),
            ObjectNode(object) => format!("Void Object: {}", object.object_type),
        };
        let shape = match void_node {
            EventNode(_) => "ellipse",
            ObjectNode(_) => "box",
        };
        // Prefix with "void_" to ensure unique node IDs in DOT
        let void_node_id = format!("void_{}", void_id);
        node_stmts.push(stmt!(node!(
            esc &void_node_id;
            attr!("label", esc &label),
            attr!("shape", shape),
            attr!("color", "blue"),
            attr!("fontcolor", "blue")
        )));

        // Similarly, add edges from void nodes if any
        // Note: Since void edges are handled separately, we assume void nodes don't have connections here
    }

    // Step 6: Add void edges (in blue) if they exist
    for (&void_edge_id, void_edge) in &alignment.void_edges {
        let edge_label = match void_edge.edge_type {
            EdgeType::DF => "DF",
            EdgeType::O2O => "O2O",
            EdgeType::E2O => "E2O",
        };
        // Prefix with "void_edge_" to ensure unique edge IDs in DOT
        // let void_edge_from = format!("{}", void_edge.from);
        // let void_edge_to = format!("{}", void_edge.to);
        // 
        // get edge from and to node names in graph c2
        let void_edge_from = match alignment.node_mapping.get(&void_edge.from).unwrap() {
            NodeMapping::VoidNode(_, void_node_id) => format!("void_{}", void_node_id),
            NodeMapping::RealNode(_, c2_id) => format!("{}", c2.nodes.get(c2_id).unwrap().id()),
        };
        
        let void_edge_to = match alignment.node_mapping.get(&void_edge.to).unwrap() {
            NodeMapping::VoidNode(_, void_node_id) => format!("void_{}", void_node_id),
            NodeMapping::RealNode(_, c2_id) => format!("{}", c2.nodes.get(c2_id).unwrap().id()),
        };
        
        edge_stmts.push(stmt!(edge!(
            node_id!(&void_edge_from) => node_id!(&void_edge_to);
            attr!("label", esc edge_label),
            attr!("color", "blue")
        )));
    }

    // Step 7: Combine all statements
    let mut global_graph_options = vec![stmt!(attr!("rankdir", "LR"))];
    if let Some(dpi_fac) = dpi_factor {
        global_graph_options.push(stmt!(attr!("dpi", (dpi_fac * 96.0))));
    }

    graph!(
        strict di id!(esc Uuid::new_v4()),
        vec![
            global_graph_options,
            node_stmts,
            edge_stmts
        ]
        .into_iter()
        .flatten()
        .collect()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oc_case::case::{EdgeType, Event, Node, Object};
    use crate::oc_case::visualization::export_case_graph_image;

    #[test]
    fn test_export_c2_with_alignment_visualization() {
        // Create c1 CaseGraph
        let mut B = CaseGraph::new();

        // Add nodes to c1
        let event1 = Node::EventNode(Event {
            id: 1,
            event_type: "A".to_string(),
        });
        let event2 = Node::EventNode(Event {
            id: 2,
            event_type: "B".to_string(),
        });
        let object1 = Node::ObjectNode(Object {
            id: 3,
            object_type: "Person".to_string(),
        });
        B.add_node(event1.clone());
        B.add_node(event2.clone());
        B.add_node(object1.clone());

        // Add edges to c1
        let edge1 = CaseEdge::new(1, 1, 2, EdgeType::DF); // A -> B
        let edge2 = CaseEdge::new(2, 2, 3, EdgeType::E2O); // B -> Person
        B.add_edge(edge1.clone());
        B.add_edge(edge2.clone());

        // Create c2 CaseGraph
        let mut A = CaseGraph::new();

        // Add nodes to c2
        let event3 = Node::EventNode(Event {
            id: 4,
            event_type: "A".to_string(),
        });
        let event4 = Node::EventNode(Event {
            id: 5,
            event_type: "B".to_string(),
        });
        let object2 = Node::ObjectNode(Object {
            id: 6,
            object_type: "Person".to_string(),
        });
        let object3 = Node::ObjectNode(Object {
            id: 7,
            object_type: "Device".to_string(),
        });
        A.add_node(event3.clone());
        A.add_node(event4.clone());
        A.add_node(object2.clone());
        A.add_node(object3.clone());

        // Add edges to c2
        let edge3 = CaseEdge::new(101, 4, 5, EdgeType::DF); // A -> B
        let edge4 = CaseEdge::new(102, 5, 6, EdgeType::E2O); // B -> Person
        let edge5 = CaseEdge::new(103, 5, 7, EdgeType::E2O); // B -> Device
        A.add_edge(edge3.clone());
        A.add_edge(edge4.clone());
        A.add_edge(edge5.clone());

        // Perform alignment
        let alignment = CaseAlignment::align_mip(&A, &B);
        println!("alignment cost: {:?}", alignment.total_cost().unwrap());

        // Export visualization
        // This will create "c2_with_alignment.png" in the current directory
        export_c2_with_alignment_image(
            &B,
            &alignment,
            "c2_with_alignment.png",
            Format::Png,
            Some(2.0),
        )
        .expect("Failed to export visualization.");


        export_case_graph_image(&A, "c1.png", Format::Png, Some(2.0)).unwrap();
        export_case_graph_image(&B, "c2.png", Format::Png, Some(2.0)).unwrap();
    }
}
