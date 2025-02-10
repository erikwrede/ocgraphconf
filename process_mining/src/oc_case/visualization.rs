use crate::oc_case::case::Node::{EventNode, ObjectNode};
use crate::oc_case::case::{CaseGraph, Edge as CaseEdge, EdgeType};
use graphviz_rust::{
    cmd::Format,
    dot_generator::{attr, edge, graph, id, node, node_id, stmt},
    dot_structures::*,
    printer::{DotPrinter, PrinterContext},
};
use std::{fs::File, io::Write};
use uuid::Uuid;

/// Export the image of a `CaseGraph`
pub fn export_case_graph_image<P: AsRef<std::path::Path>>(
    graph: &CaseGraph,
    path: P,
    format: Format,
    dpi_factor: Option<f32>,
) -> Result<(), std::io::Error> {
    let g = export_case_graph_to_dot_graph(graph, dpi_factor);
    let out = graphviz_rust::exec(g, &mut PrinterContext::default(), vec![format.into()])?;
    let mut f = File::create(path)?;
    f.write_all(&out)?;
    Ok(())
}

/// Export a `CaseGraph` to a DOT graph (used in Graphviz)
pub fn export_case_graph_to_dot_graph(graph: &CaseGraph, dpi_factor: Option<f32>) -> Graph {
    let node_stmts: Vec<_> = graph
        .nodes
        .values()
        .map(|node| match node {
            EventNode(event) => {
                stmt!(node!(esc format!("{}", event.id);
                    attr!("label", esc &event.event_type),
                    attr!("shape", "ellipse")
                ))
            }
            ObjectNode(object) => {
                stmt!(node!(esc format!("{}", object.id);
                    attr!("label", esc &object.object_type),
                    attr!("shape", "box")
                ))
            }
        })
        .collect();

    let edge_stmts: Vec<_> = graph
        .edges
        .values()
        .map(|edge| {
            let edge_label = match edge.edge_type {
                EdgeType::DF => "DF",
                EdgeType::O2O => "O2O",
                EdgeType::E2O => "E2O",
            };
            stmt!(edge!(node_id!(esc format!("{}", edge.from)) => node_id!(esc format!("{}", edge.to));
                attr!("label", esc edge_label)
            ))
        })
        .collect();

    let mut global_graph_options = vec![stmt!(attr!("rankdir", "LR"))];
    if let Some(dpi_fac) = dpi_factor {
        global_graph_options.push(stmt!(attr!("dpi", (dpi_fac * 96.0))))
    }

    graph!(strict di id!(esc Uuid::new_v4()),
        vec![global_graph_options, node_stmts, edge_stmts]
            .into_iter()
            .flatten()
            .collect()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oc_case::case::{Event, Object};
    #[test]
    fn test_case_graph_export() {
        let mut graph = CaseGraph::new();
        // Add nodes
        let event = Event {
            id: 1,
            event_type: "A".to_string(),
        };
        let object = Object {
            id: 2,
            object_type: "Person".to_string(),
        };
        graph.add_node(EventNode(event));
        graph.add_node(ObjectNode(object));

        // Add an edge
        let edge = CaseEdge::new(1, 1, 2, EdgeType::E2O);
        graph.add_edge(edge);

        // Export the graph
        export_case_graph_image(&graph, "test_case_graph.png", Format::Png, Some(2.0)).unwrap();
    }
}
