use crate::oc_align::align_case::CaseAlignment;
use crate::oc_case::case::{CaseGraph, CaseStats, Edge, EdgeType, Event, Node, Object};
use crate::oc_petri_net::marking::{Binding, Marking};
use crate::oc_petri_net::oc_petri_net::ObjectCentricPetriNet;
use std::cmp::PartialEq;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct SearchNode {
    marking: Marking,
    partial_case: CaseGraph,
    min_cost: f64,
    most_recent_event_id: Option<usize>,
    action: SearchNodeAction,
    partial_case_stats: CaseStats,
}

impl SearchNode {
    fn new(
        marking: Marking,
        partial_case: CaseGraph,
        min_cost: f64,
        most_recent_event_id: Option<usize>,
        action: SearchNodeAction,
    ) -> Self {
        SearchNode {
            marking,
            partial_case_stats: partial_case.get_case_stats(),
            partial_case,
            min_cost,
            most_recent_event_id,
            action,
        }
    }
    
    /// Manually pass the case stats, if you already know them from the changes of the previous node
    fn new_with_stats(
        marking: Marking,
        partial_case: CaseGraph,
        min_cost: f64,
        most_recent_event_id: Option<usize>,
        action: SearchNodeAction,
        partial_case_stats: CaseStats,
    ) -> Self {
        SearchNode {
            marking,
            partial_case_stats,
            partial_case,
            min_cost,
            most_recent_event_id,
            action,
        }
    }
}

#[derive(Debug, Clone)]
enum SearchNodeAction {
    FireTransition(Uuid, Binding),
    AddToken(Uuid),
    // used for the initial node
    VOID,
}

impl SearchNodeAction {
    fn fire_transition(transition_id: Uuid, binding: Binding) -> Self {
        SearchNodeAction::FireTransition(transition_id, binding)
    }

    fn add_token(place_id: Uuid) -> Self {
        SearchNodeAction::AddToken(place_id)
    }

    fn is_pre_firing(&self) -> bool {
        match self {
            SearchNodeAction::FireTransition(_, _) => false,
            _ => true,
        }
    }

    fn log(&self, object_centric_petri_net: &ObjectCentricPetriNet) {
        match self {
            SearchNodeAction::FireTransition(transition_id, binding) => {
                println!(
                    "Firing transition: {} with binding: {:?}",
                    object_centric_petri_net
                        .get_transition(transition_id)
                        .unwrap()
                        .name,
                    binding
                );
            }
            SearchNodeAction::AddToken(object_id) => {
                println!(
                    "Adding token to place: {}",
                    object_centric_petri_net
                        .get_place(object_id)
                        .unwrap()
                        .name
                        .clone()
                        .unwrap_or("no_name".to_string())
                );
            }
            SearchNodeAction::VOID => {
                println!("Initial node");
            }
        }
    }
}

struct ModelCaseChecker {
    object_id_mapping: HashMap<usize, usize>,
}
impl ModelCaseChecker {
    fn new() -> Self {
        ModelCaseChecker {
            object_id_mapping: HashMap::new(),
        }
    }

    fn branch_and_bound<'a>(
        &mut self,
        model: Arc<ObjectCentricPetriNet>,
        query_case: &'a CaseGraph,
        initial_marking: Marking,
    ) -> Option<SearchNode> {
        println!("starting");
        //let mut global_upper_bound = f64::INFINITY;
        let mut global_upper_bound = 50.0;
        let mut global_lower_bound = 0.0;
        let mut best_node: Option<SearchNode> = None;

        let query_case_stats = query_case.get_case_stats();
        query_case_stats.pretty_print_stats();

        let mut open_list: Vec<SearchNode> = vec![SearchNode::new(
            initial_marking,
            CaseGraph::new(),
            0.0,
            None,
            SearchNodeAction::VOID,
        )];
        println!("starting");

        while let Some(mut current_node) = open_list.pop() {
            println!("Number of open nodes: {}", open_list.len());
            println!("=====================");
            if current_node.min_cost >= global_upper_bound {
                continue;
            }


            open_list.sort_by(|a, b| a.min_cost.partial_cmp(&b.min_cost).unwrap());
            //current_node.partial_case_stats.pretty_print_stats();
            println!("Solving node with min cost: {}", current_node.min_cost);
            current_node.action.log(&model);
            // print all the keys in a single line
            let events = current_node.partial_case_stats.query_event_counts.keys();
            println!("Events: {:?}", events);
                
            let alignment = CaseAlignment::align_mip(&current_node.partial_case, query_case);
            println!("Alignment cost: {}", alignment.total_cost().unwrap_or(f64::INFINITY));

            let alignment_cost = alignment.total_cost().unwrap_or(f64::INFINITY);

            if current_node.marking.is_final_has_tokens() {
                // temporarily throw an error here so everything is stopped
                // now output a lot of info such as a string repr of the current case we found and the cost etc
                print!("Final marking reached");
                print!("Cost: {}", alignment_cost);
                panic!("Final marking reached");

                // Limit the scope of the mutable borrow using a separate block
                if alignment_cost < global_upper_bound {
                    // log that new best bound has been found
                    println!("New best bound found: {}", alignment_cost);

                    global_upper_bound = alignment_cost;
                    best_node = Some(current_node.clone());
                    open_list
                        .iter()
                        .filter(|node| node.min_cost >= global_upper_bound)
                        .for_each(|node| {
                            // log that node has been pruned
                            println!("Pruned node with cost: {}", node.min_cost);
                        });
                }
            }

            if (alignment_cost > global_upper_bound) {
                println!("Pruned node with cost: {}", alignment_cost);
                continue;
            }
            println!("finding children");
            let children = self.generate_children(&current_node, &model, &query_case_stats);
            for mut child in children {
                if child.min_cost < global_upper_bound {
                    open_list.push(child);
                } else {
                    //println!("Pruned node before exploring with min cost: {}", child.min_cost);
                }
            }
            println!("sorting list");
            // now sort the open_list so that we expand first on the lowest min cost node
            open_list.sort_by(|a, b| a.min_cost.partial_cmp(&b.min_cost).unwrap());
        }

        best_node
    }

    fn calculate_min_cost(
        &self,
        query_case_stats: &CaseStats,
        partial_case_stats: &CaseStats,
    ) -> f64 {
        let mut total_cost = 0.0;

        for (event_type, &partial_count) in &partial_case_stats.query_event_counts {
            let query_count = query_case_stats.query_event_counts.get(event_type).unwrap_or(&0);
            total_cost += (partial_count as f64 - *query_count as f64).max(0.0);
        }

        for (object_type, &partial_count) in &partial_case_stats.query_object_counts {
            let query_count = query_case_stats.query_object_counts.get(object_type).unwrap_or(&0);
            total_cost += (partial_count as f64 - *query_count as f64).max(0.0);
        }

        for (edge_type, &partial_count) in &partial_case_stats.query_edge_counts {
            let query_count = query_case_stats.query_edge_counts.get(edge_type).unwrap_or(&0);
            total_cost += (partial_count as f64 - *query_count as f64).max(0.0);
        }

        total_cost
    }

    fn generate_children<'a>(
        &mut self,
        node: &SearchNode,
        model: &ObjectCentricPetriNet,
        query_case_stats: &CaseStats,
    ) -> Vec<SearchNode> {
        let mut children = Vec::new();

        // only add tokens to initial places, if the node is the initial node or follows a add token action node
        println!("GEtting initial places");
        if (node.action.is_pre_firing()) {
            for place in model.get_initial_places() {
                let mut new_marking = node.marking.clone();
                let token_ids = new_marking.add_initial_token_count(&place.id, 1);

                let mut new_partial_case = node.partial_case.clone();

                let new_object = Node::Object(Object {
                    id: token_ids[0],
                    object_type: place.object_type.clone(),
                });

                new_partial_case.add_node(new_object);
                
                let mut new_partial_case_stats = node.partial_case_stats.clone();
                new_partial_case_stats.query_object_counts
                    .entry(place.object_type.clone())
                    .and_modify(|e| *e += 1)
                    .or_insert(1);

                let min_cost = self.calculate_min_cost(&query_case_stats, &new_partial_case_stats);

                children.push(SearchNode::new_with_stats(
                    new_marking,
                    new_partial_case,
                    min_cost,
                    None,
                    SearchNodeAction::add_token(place.id.clone()),
                    new_partial_case_stats
                ));
            }
        }

        println!("Getting transitions");
        
        //
        if (!node.action.is_pre_firing()) {
                
        }
        for transition in model.transitions.values() {
            let firing_combinations = node.marking.get_firing_combinations(transition);

            firing_combinations.iter().for_each(|combination| {
                let mut new_marking = node.marking.clone();
                let mut new_partial_case = node.partial_case.clone();

                let mut most_recent_event_id = node.most_recent_event_id;

                new_marking.fire_transition(transition, combination);
                let mut new_partial_case_stats = node.partial_case_stats.clone();

                if (!transition.silent) {
                    let event_id = new_partial_case.nodes.len() + 1;
                    let new_event = Node::Event(Event {
                        id: event_id,
                        event_type: transition.name.clone(),
                    });
                    new_partial_case.add_node(new_event);

                    combination
                        .object_binding_info
                        .values()
                        .for_each(|binding_info| {
                            binding_info.tokens.iter().for_each(|token_id| {
                                new_partial_case.add_edge(Edge::new(
                                    new_partial_case.edges.len() + 1,
                                    event_id,
                                    token_id.id,
                                    EdgeType::E2O,
                                ));
                                new_partial_case_stats.query_edge_counts
                                    .entry(EdgeType::E2O)
                                    .and_modify(|e| *e += 1)
                                    .or_insert(1);
                            })
                        });

                    if let Some(prev_event_id) = node.most_recent_event_id {
                        new_partial_case.add_edge(Edge::new(
                            new_partial_case.edges.len() + 1,
                            prev_event_id,
                            event_id,
                            EdgeType::DF,
                        ));
                    }
                    
                    new_partial_case_stats.query_event_counts
                        .entry(transition.name.clone())
                        .and_modify(|e| *e += 1)
                        .or_insert(1);
                    
                    new_partial_case_stats.query_edge_counts
                        .entry(EdgeType::DF)
                        .and_modify(|e| *e += 1)
                        .or_insert(1);
                    
                    most_recent_event_id = Some(event_id);
                }

                let new_cost = self.calculate_min_cost(&query_case_stats, &new_partial_case_stats);

                children.push(SearchNode::new_with_stats(
                    new_marking,
                    new_partial_case,
                    new_cost,
                    most_recent_event_id,
                    SearchNodeAction::fire_transition(transition.id, combination.clone()),
                    new_partial_case_stats
                ));
            });
        }

        children
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oc_petri_net::initialize_ocpn_from_json;
    use std::fs;

    #[test]
    fn test_basic_alignment() {
        // Create an Object Centric Petri Net
        let mut petri_net = ObjectCentricPetriNet::new();

        // Define places
        let p1 = petri_net.add_place(
            Some("Start".to_string()),
            "ObjType".to_string(),
            true,
            false,
        );
        let p2 = petri_net.add_place(
            Some("Middle".to_string()),
            "ObjType".to_string(),
            false,
            false,
        );
        let p3 = petri_net.add_place(Some("End".to_string()), "ObjType".to_string(), false, true);

        // Define transitions
        let t1 = petri_net.add_transition("T1".to_string(), None, false);
        let t2 = petri_net.add_transition("T2".to_string(), None, false);

        // Connect places and transitions with arcs
        petri_net.add_input_arc(p1.id, t1.id, false, 1);
        petri_net.add_output_arc(t1.id, p2.id, false, 1);
        petri_net.add_input_arc(p2.id, t2.id, false, 1);
        petri_net.add_output_arc(t2.id, p3.id, false, 1);

        let initial_marking = Marking::new(petri_net.clone());
        // Wrap the petri net in Arc to match branch_and_bound signature
        let petri_net_arc = Arc::new(petri_net);

        // Create a CaseGraph representing the query case
        let mut query_case = CaseGraph::new();

        // Add nodes corresponding to the events in the query case
        let event1 = Node::Event(Event {
            id: 1,
            event_type: "T1".to_string(),
        });
        let event2 = Node::Event(Event {
            id: 2,
            event_type: "T2".to_string(),
        });
        query_case.add_node(event1);
        query_case.add_node(event2);

        // Connect the events with a direct follows edge
        query_case.add_edge(Edge::new(1, 1, 2, EdgeType::DF));

        // Initialize ModelCaseChecker
        let mut checker = ModelCaseChecker::new();

        // Use branch_and_bound to find alignment
        let result = checker.branch_and_bound(petri_net_arc, &query_case, initial_marking);

        // Validate if a result is found
        assert!(result.is_some(), "Failed to find a valid alignment");

        let best_node = result.unwrap();
        let total_cost = best_node.min_cost;

        // Print the results for debugging
        println!("Best alignment cost: {}", total_cost);
        //best_node.partial_case.print_mappings();
    }

    #[test]
    fn large_petri_net() {
        let json_data = fs::read_to_string("./src/oc_petri_net/util/oc_petri_net.json").unwrap();
        let ocpn = initialize_ocpn_from_json(&json_data);

        let initial_marking = Marking::new(ocpn.clone());
        // Wrap the petri net in Arc to match branch_and_bound signature
        let petri_net_arc = Arc::new(ocpn);

        // Create a CaseGraph representing the query case
        let mut query_case = CaseGraph::new();

        // Add nodes corresponding to the events in the query case
        let event1 = Node::Event(Event {
            id: 1,
            event_type: "A".to_string(),
        });

        query_case.add_node(event1);

        // Initialize ModelCaseChecker
        let mut checker = ModelCaseChecker::new();

        // Use branch_and_bound to find alignment
        let result = checker.branch_and_bound(petri_net_arc, &query_case, initial_marking);

        // Validate if a result is found
        assert!(result.is_some(), "Failed to find a valid alignment");

        let best_node = result.unwrap();
        let total_cost = best_node.min_cost;
    }

    #[test]
    fn test_alignment_with_void_operations() {
        // Create a slightly more complex Petri net with optional paths (void scenarios)

        let mut petri_net = ObjectCentricPetriNet::new();

        // Define places
        let p1 = petri_net.add_place(
            Some("Start".to_string()),
            "ObjType".to_string(),
            true,
            false,
        );
        let p2 = petri_net.add_place(
            Some("Middle".to_string()),
            "ObjType".to_string(),
            false,
            false,
        );
        let p3 = petri_net.add_place(Some("End".to_string()), "ObjType".to_string(), false, true);
        let p4 = petri_net.add_place(
            Some("Optional".to_string()),
            "ObjType".to_string(),
            false,
            false,
        );

        // Define transitions
        let t1 = petri_net.add_transition("T1".to_string(), None, false);
        let t2 = petri_net.add_transition("T2".to_string(), None, false);
        let t3 = petri_net.add_transition("T3".to_string(), None, true); // Silent transition

        // Connect places and transitions with arcs
        petri_net.add_input_arc(p1.id, t1.id, false, 1);
        petri_net.add_output_arc(t1.id, p2.id, false, 1);
        petri_net.add_input_arc(p2.id, t2.id, false, 1);
        petri_net.add_output_arc(t2.id, p3.id, false, 1);
        petri_net.add_input_arc(p2.id, t3.id, true, 1);
        petri_net.add_output_arc(t3.id, p4.id, false, 1);

        let initial_marking = Marking::new(petri_net.clone());
        // Wrap the petri net in Arc
        let petri_net_arc = Arc::new(petri_net);

        // Create a CaseGraph representing the query case (missing optional path)
        let mut query_case = CaseGraph::new();

        // Add nodes corresponding to the events in the query case
        let event1 = Node::Event(Event {
            id: 1,
            event_type: "T1".to_string(),
        });
        let event2 = Node::Event(Event {
            id: 2,
            event_type: "T2".to_string(),
        });
        query_case.add_node(event1);
        query_case.add_node(event2);

        // Connect the events with a direct follows edge
        query_case.add_edge(Edge::new(2, 1, 2, EdgeType::DF));

        // Initialize ModelCaseChecker
        let mut checker = ModelCaseChecker::new();

        // Use branch_and_bound to find alignment
        let result = checker.branch_and_bound(petri_net_arc, &query_case, initial_marking);

        // Validate if a result is found
        assert!(result.is_some(), "Failed to find a valid alignment");

        let best_node = result.unwrap();
        let total_cost = best_node.min_cost;

        // Print the results for debugging
        println!(
            "Best alignment cost with optionally void edges: {}",
            total_cost
        );
        //best_node.partial_case.print_mappings();
    }
}
