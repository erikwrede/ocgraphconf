use crate::oc_align::align_case::CaseAlignment;
use crate::oc_case::case::{CaseGraph, Edge, EdgeType, Event, Node, Object};
use crate::oc_petri_net::marking::Marking;
use crate::oc_petri_net::oc_petri_net::ObjectCentricPetriNet;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
struct SearchNode {
    marking: Marking,
    partial_case: CaseGraph,
    min_cost: f64,
    most_recent_event_id: Option<usize>,
}

impl SearchNode {
    fn new(
        marking: Marking,
        partial_case: CaseGraph,
        min_cost: f64,
        most_recent_event_id: Option<usize>,
    ) -> Self {
        SearchNode {
            marking,
            partial_case,
            min_cost,
            most_recent_event_id,
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

        let mut open_list: Vec<SearchNode> = vec![SearchNode::new(
            initial_marking,
            CaseGraph::new(),
            0.0,
            None,
        )];
        println!("starting");

        while let Some(mut current_node) = open_list.pop() {
            println!("Number of open nodes: {}", open_list.len());
            if current_node.min_cost >= global_upper_bound {
                continue;
            }

            open_list.sort_by(|a, b| a.min_cost.partial_cmp(&b.min_cost).unwrap());

            let alignment = CaseAlignment::align_mip(&current_node.partial_case, query_case);

            let alignment_cost = alignment.total_cost().unwrap_or(f64::INFINITY);

            if current_node.marking.is_final_has_tokens() {
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

            let children = self.generate_children(&current_node, &model, query_case);
            for mut child in children {
                if child.min_cost < global_upper_bound {
                    open_list.push(child);
                }
            }
        }

        best_node
    }

    fn calculate_min_cost(&mut self, node: &SearchNode, query_case: &CaseGraph) -> f64 {
        let alignment = CaseAlignment::align_mip(&node.partial_case, query_case);
        alignment.total_cost().unwrap_or(f64::INFINITY)
    }

    fn generate_children<'a>(
        &mut self,
        node: &SearchNode,
        model: &ObjectCentricPetriNet,
        query_case: &'a CaseGraph,
    ) -> Vec<SearchNode> {
        let mut children = Vec::new();

        for place in model.get_initial_places() {
            let mut new_marking = node.marking.clone();
            let token_ids = new_marking.add_initial_token_count(&place.id, 1);

            let mut new_partial_case = node.partial_case.clone();

            let new_object = Node::Object(Object {
                id: token_ids[0],
                object_type: place.object_type.clone(),
            });

            new_partial_case.add_node(new_object);

            let new_cost = self.calculate_min_cost(
                &SearchNode::new(
                    new_marking.clone(),
                    new_partial_case.clone(),
                    0.0,
                    node.most_recent_event_id,
                ),
                query_case,
            );

            children.push(SearchNode::new(new_marking, new_partial_case, new_cost, None));
        }

        for transition in model.transitions.values() {
            let firing_combinations = node.marking.get_firing_combinations(transition);

            firing_combinations.iter().for_each(|combination| {
                let mut new_marking = node.marking.clone();
                let mut new_partial_case = node.partial_case.clone();
                
                let mut most_recent_event_id = node.most_recent_event_id;
                
                new_marking.fire_transition(transition, combination);
                

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
                    most_recent_event_id = Some(event_id);
                }
                

                let new_cost = self.calculate_min_cost(
                    &SearchNode::new(new_marking.clone(), new_partial_case.clone(), 0.0, most_recent_event_id),
                    query_case,
                );

                children.push(SearchNode::new(new_marking, new_partial_case, new_cost, most_recent_event_id));
            });
        }

        children
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use crate::oc_petri_net::initialize_ocpn_from_json;
    use super::*;

    #[test]
    fn test_basic_alignment() {
        // Create an Object Centric Petri Net
        let mut petri_net = ObjectCentricPetriNet::new();

        // Define places
        let p1 = petri_net.add_place(Some("Start".to_string()), "ObjType".to_string(), true, false);
        let p2 = petri_net.add_place(Some("Middle".to_string()), "ObjType".to_string(), false, false);
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
        let event1 = Node::Event(Event { id: 1, event_type: "T1".to_string() });
        let event2 = Node::Event(Event { id: 2, event_type: "T2".to_string() });
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
        let event1 = Node::Event(Event { id: 1, event_type: "A".to_string() });

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
        let p1 = petri_net.add_place(Some("Start".to_string()), "ObjType".to_string(), true, false);
        let p2 = petri_net.add_place(Some("Middle".to_string()), "ObjType".to_string(), false, false);
        let p3 = petri_net.add_place(Some("End".to_string()), "ObjType".to_string(), false, true);
        let p4 = petri_net.add_place(Some("Optional".to_string()), "ObjType".to_string(), false, false);

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
        let event1 = Node::Event(Event { id: 1, event_type: "T1".to_string() });
        let event2 = Node::Event(Event { id: 2, event_type: "T2".to_string() });
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
        println!("Best alignment cost with optionally void edges: {}", total_cost);
        //best_node.partial_case.print_mappings();
    }
}
