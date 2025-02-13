use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;
use crate::oc_case::case::{CaseGraph, Edge, EdgeType, Event, Node, Object};

// Implement Serialize and Deserialize on Event
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SerializableEvent {
    pub id: usize,
    pub event_type: String,
}

impl From<Event> for SerializableEvent {
    fn from(event: Event) -> Self {
        SerializableEvent {
            id: event.id,
            event_type: event.event_type,
        }
    }
}

impl From<SerializableEvent> for Event {
    fn from(serializable_event: SerializableEvent) -> Self {
        Event {
            id: serializable_event.id,
            event_type: serializable_event.event_type,
        }
    }
}

// Implement Serialize and Deserialize on Object
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SerializableObject {
    pub id: usize,
    pub object_type: String,
}

impl From<Object> for SerializableObject {
    fn from(object: Object) -> Self {
        SerializableObject {
            id: object.id,
            object_type: object.object_type,
        }
    }
}

impl From<SerializableObject> for Object {
    fn from(serializable_object: SerializableObject) -> Self {
        Object {
            id: serializable_object.id,
            object_type: serializable_object.object_type,
        }
    }
}

// Implement Serialize and Deserialize on Node
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", content = "data")]
pub enum SerializableNode {
    EventNode(SerializableEvent),
    ObjectNode(SerializableObject),
}

impl From<Node> for SerializableNode {
    fn from(node: Node) -> Self {
        match node {
            Node::EventNode(event) => SerializableNode::EventNode(event.into()),
            Node::ObjectNode(object) => SerializableNode::ObjectNode(object.into()),
        }
    }
}

impl From<SerializableNode> for Node {
    fn from(serializable_node: SerializableNode) -> Self {
        match serializable_node {
            SerializableNode::EventNode(event) => Node::EventNode(event.into()),
            SerializableNode::ObjectNode(object) => Node::ObjectNode(object.into()),
        }
    }
}

// Implement Serialize and Deserialize on Edge
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SerializableEdge {
    pub id: usize,
    pub from: usize,
    pub to: usize,
    pub edge_type: EdgeType,
}

impl From<Edge> for SerializableEdge {
    fn from(edge: Edge) -> Self {
        SerializableEdge {
            id: edge.id,
            from: edge.from,
            to: edge.to,
            edge_type: edge.edge_type,
        }
    }
}

impl From<SerializableEdge> for Edge {
    fn from(serializable_edge: SerializableEdge) -> Self {
        Edge {
            id: serializable_edge.id,
            from: serializable_edge.from,
            to: serializable_edge.to,
            edge_type: serializable_edge.edge_type,
        }
    }
}

// Implement Serialize and Deserialize on CaseGraph
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SerializableCaseGraph {
    pub nodes: HashMap<usize, SerializableNode>,
    pub edges: HashMap<usize, SerializableEdge>,
    pub adjacency: HashMap<usize, Vec<usize>>,
    pub counter: usize,
}

impl From<CaseGraph> for SerializableCaseGraph {
    fn from(graph: CaseGraph) -> Self {
        SerializableCaseGraph {
            nodes: graph.nodes.into_iter().map(|(id, node)| (id, node.into())).collect(),
            edges: graph.edges.into_iter().map(|(id, edge)| (id, edge.into())).collect(),
            adjacency: graph.adjacency,
            counter: graph.counter,
        }
    }
}

impl From<SerializableCaseGraph> for CaseGraph {
    fn from(serializable_graph: SerializableCaseGraph) -> Self {
        CaseGraph {
            nodes: serializable_graph.nodes.into_iter().map(|(id, node)| (id, node.into())).collect(),
            edges: serializable_graph.edges.into_iter().map(|(id, edge)| (id, edge.into())).collect(),
            adjacency: serializable_graph.adjacency,
            counter: serializable_graph.counter,
        }
    }
}

// Functions to serialize and deserialize the CaseGraph
pub fn serialize_case_graph(graph: &CaseGraph) -> String {
    let serializable_graph: SerializableCaseGraph = graph.clone().into();
    serde_json::to_string(&serializable_graph).expect("Failed to serialize CaseGraph")
}

pub fn deserialize_case_graph(serialized_graph: &str) -> CaseGraph {
    let serializable_graph: SerializableCaseGraph = serde_json::from_str(serialized_graph).expect("Failed to deserialize CaseGraph");
    serializable_graph.into()
}