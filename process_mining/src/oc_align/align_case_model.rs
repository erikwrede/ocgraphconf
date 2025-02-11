use crate::oc_align::align_case::CaseAlignment;
use crate::oc_align::util::reachability_cache::ReachabilityCache;
use crate::oc_align::visualization::case_visual::export_c2_with_alignment_image;
use crate::oc_case::case::{CaseGraph, CaseStats, Edge, EdgeType, Event, Node, Object};
use crate::oc_case::visualization::export_case_graph_image;
use crate::oc_petri_net::marking::{Binding, Marking};
use crate::oc_petri_net::oc_petri_net::ObjectCentricPetriNet;
use crate::type_storage::TYPE_STORAGE;
use graphviz_rust::cmd::Format;
use std::cmp::PartialEq;
use std::collections::{HashMap, HashSet};
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

    fn transition_id(&self) -> Option<Uuid> {
        match self {
            SearchNodeAction::FireTransition(transition_id, _) => Some(*transition_id),
            _ => None,
        }
    }

    fn log(&self, object_centric_petri_net: Arc<ObjectCentricPetriNet>) {
        match self {
            SearchNodeAction::FireTransition(transition_id, binding) => {
                // map objectbindinginfo to object names, token count
                let object_names: Vec<String> = binding
                    .object_binding_info
                    .values()
                    .map(|binding_info| {
                        let object_name = binding_info.object_type.clone();
                        let token_count = binding_info.tokens.len();
                        format!("{}: {}", object_name, token_count)
                    })
                    .collect();

                println!(
                    "Firing transition: {} with binding: {:?}",
                    object_centric_petri_net
                        .get_transition(transition_id)
                        .unwrap()
                        .name,
                    object_names
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
    token_graph_id_mapping: HashMap<usize, usize>,
    reachability_cache: ReachabilityCache,
    model: Arc<ObjectCentricPetriNet>,
    shortest_case: Option<CaseGraph>,
    model_transitions: HashSet<String>,
}
impl ModelCaseChecker {
    fn new(model: Arc<ObjectCentricPetriNet>) -> Self {
        ModelCaseChecker {
            token_graph_id_mapping: HashMap::new(),
            reachability_cache: ReachabilityCache::new(model.clone()),
            model_transitions: model.transitions.values().map(|t| t.name.clone()).collect(),
            model,
            shortest_case: None,
        }
    }

    fn new_with_shortest_case(model: Arc<ObjectCentricPetriNet>, shortest_case: CaseGraph) -> Self {
        ModelCaseChecker {
            token_graph_id_mapping: HashMap::new(),
            reachability_cache: ReachabilityCache::new(model.clone()),
            model_transitions: model.transitions.values().map(|t| t.name.clone()).collect(),
            model,
            shortest_case: Some(shortest_case),
        }
    }

    fn initialize_node_with_initial_places(
        &mut self,
        query_case_stats: &CaseStats,
        mut new_marking: Marking,
        static_cost: f64,
    ) -> SearchNode {
        let mut new_partial_case = CaseGraph::new();

        // Get the initial places from the model
        let initial_places: HashSet<String> = self
            .model
            .get_initial_places()
            .iter()
            .map(|p| p.object_type.clone())
            .collect();

        // Initialize marking and case from query_case_stats, only for initial places
        for (object_type, count) in &query_case_stats.query_object_counts {
            if initial_places.contains(object_type) {
                for _ in 0..*count {
                    // Find the initial place with the same object type
                    let place = self
                        .model
                        .get_initial_places()
                        .iter()
                        .find(|p| &p.object_type == object_type)
                        .expect("Expected matching initial place for given object type")
                        .clone();

                    // Add token to this initial place in the marking
                    let token_ids = new_marking.add_initial_token_count(&place.id, 1);

                    // Create a new object node and add it to the case graph
                    let object_id = new_partial_case.get_new_id();
                    let new_object = Node::ObjectNode(Object {
                        id: object_id.clone(),
                        object_type: object_type.clone(),
                    });
                    new_partial_case.add_node(new_object);

                    // Map the token to object id
                    self.token_graph_id_mapping.insert(token_ids[0], object_id);
                }
            }
        }

        let min_cost = self.calculate_min_cost(
            &query_case_stats,
            &new_partial_case.get_case_stats(),
            static_cost,
        );
        println!("Starting Min Cost: {}", min_cost);
        // Calculate the minimum cost using the initialized stats
        let min_cost = 0;

        // Create and return the initialized SearchNode
        SearchNode::new(
            new_marking,
            new_partial_case,
            0 as f64,
            None,
            SearchNodeAction::VOID,
        )
    }

    fn branch_and_bound<'a>(
        &mut self,
        query_case: &'a CaseGraph,
        initial_marking: Marking,
    ) -> Option<SearchNode> {
        //println!("starting");
        //let mut global_upper_bound = f64::INFINITY;
        let mut global_upper_bound = 5000.0;
        let mut global_lower_bound = 0.0;
        let mut best_node: Option<SearchNode> = None;

        println!(
            "Types in the net: {:?}",
            self.model
                .get_initial_places()
                .iter()
                .map(|p| p.object_type.clone())
                .collect::<HashSet<_>>()
        );

        let mut static_void_cost =
            self.calculate_query_static_costs(query_case, query_case.get_case_stats());

        let mut any_solution_found = false;

        if let Some(shortest_case) = &self.shortest_case {
            println!("Calculating initial upper bound");
            let alignment = CaseAlignment::align_mip(query_case, shortest_case);
            global_upper_bound = alignment.total_cost().unwrap_or(f64::INFINITY);
            println!("Initial upper bound: {}", global_upper_bound);

            best_node = Some(SearchNode::new(
                initial_marking.clone(),
                shortest_case.clone(),
                global_upper_bound,
                None,
                SearchNodeAction::VOID,
            ));
        }

        let query_case_stats = query_case.get_case_stats();
        query_case_stats.pretty_print_stats();

        let mut open_list: Vec<SearchNode> = vec![
            /*SearchNode::new(
                initial_marking,
                CaseGraph::new(),
                0.0,
                None,
                SearchNodeAction::VOID,
            )*/
            self.initialize_node_with_initial_places(
                &query_case_stats,
                initial_marking.clone(),
                static_void_cost,
            ),
        ];
        //println!("starting");
        let mut counter = 0;
        let mut mip_counter = 0;
        // save current time
        let mut most_recent_timestamp = std::time::Instant::now();
        while let Some(mut current_node) = open_list.pop() {
            counter += 1;
            // every 5 seconds print an update
            if most_recent_timestamp.elapsed().as_secs() >= 20 {
                most_recent_timestamp = std::time::Instant::now();
                println!("===================== Progress update =====================");
                println!("Nodes explored: {}", counter);
                println!("Nodes algined: {}", mip_counter);
                println!("Open list length: {}", open_list.len());
                println!("Current node min cost: {}", current_node.min_cost);
                println!("Global Upper Bound: {}", global_upper_bound);
                println!("---------------------");
                current_node.partial_case_stats.pretty_print_stats();

                // save an intermediate result as an image in ./intermediates
                let intermediate_graph = current_node.partial_case.clone();
                let intermediate_alignment =
                    CaseAlignment::align_mip(query_case, &intermediate_graph);
                println!(
                    "Intermediate alignment cost: {}",
                    intermediate_alignment.total_cost().unwrap_or(f64::INFINITY)
                );
                export_case_graph_image(
                    &intermediate_graph,
                    format!("./intermediates/intermediate_{}_case.png", counter).as_str(),
                    Format::Png,
                    Some(2.0),
                )
                .unwrap();

                export_c2_with_alignment_image(
                    &intermediate_graph,
                    &intermediate_alignment,
                    format!("./intermediates/intermediate_{}_alignment.png", counter).as_str(),
                    Format::Png,
                    Some(2.0),
                )
                .unwrap();
            }
            if (counter >= 800000) && false {
                println!("No solution found");
                current_node.partial_case_stats.pretty_print_stats();
                let events = current_node.partial_case_stats.query_event_counts.keys();
                println!("Events: {:?}", events);
                println!("Current node min cost: {}", current_node.min_cost);
                println!("Open list length: {}", open_list.len());
                break;
            }

            // print an update every 100 nodes

            //println!("Number of open nodes: {}", open_list.len());
            //println!("=====================");
            if current_node.min_cost >= global_upper_bound {
                continue;
            }

            //open_list.sort_by(|a, b| a.min_cost.partial_cmp(&b.min_cost).unwrap());
            //current_node.partial_case_stats.pretty_print_stats();
            //println!("Solving node with min cost: {}", current_node.min_cost);
            //current_node.action.log(self.model.clone());
            // print all the keys in a single line
            let events = current_node.partial_case_stats.query_event_counts.keys();
            //println!("Events: {:?}", events);

            let generate_children = true;
            if current_node.marking.is_final_has_tokens() {
                mip_counter += 1;
                // temporarily throw an error here so everything is stopped
                // now output a lot of info such as a string repr of the current case we found and the cost etc
                let alignment = CaseAlignment::align_mip(query_case, &current_node.partial_case);
                //println!("Alignment cost: {}", alignment.total_cost().unwrap_or(f64::INFINITY));
                /*
                if ((alignment.void_nodes.len() + alignment.void_edges.len()
                    - static_void_cost as usize)
                    <= 1)
                {
                    println!(
                        "Difference {}",
                        alignment.void_nodes.len() + alignment.void_edges.len()
                            - static_void_cost as usize
                    );
                    println!(
                        "Void nodes: {:?}",
                        alignment
                            .void_nodes
                            .values()
                            .map(|n| n.type_name())
                            .collect::<Vec<_>>()
                    );

                    // visualize alignment in file
                    export_c2_with_alignment_image(
                        &current_node.partial_case,
                        &alignment,
                        format!("./intermediates_2/alignment_{}_case.png", counter).as_str(),
                        Format::Png,
                        Some(2.0),
                    );
                }*/

                // print a list of type names of void nodes

                let alignment_cost = alignment.total_cost().unwrap_or(f64::INFINITY);
                // println!("Final marking reached after exploring {} nodes", counter);
                // println!("Cost: {}", alignment_cost);
                // println!(
                //     "fired events: {:?}",
                //     current_node.partial_case_stats.query_event_counts
                // );
                //panic!("Final marking reached");
                if (!any_solution_found) {
                    any_solution_found = true;
                    println!("First final marking reached")
                }
                // Limit the scope of the mutable borrow using a separate block
                if alignment_cost < global_upper_bound {
                    // log that new best bound has been found
                    println!("New best bound found: {}", alignment_cost);

                    global_upper_bound = alignment_cost;
                    best_node = Some(current_node.clone());
                    let len = open_list
                        .iter()
                        .filter(|node| node.min_cost >= global_upper_bound)
                        .count();

                    // println!("Number of nodes pruned due to best bound: {}", len);

                    // remove all nodes that have a cost higher than the new upper bound
                    open_list.retain(|node| node.min_cost < global_upper_bound);
                }
            }

            if (generate_children) {
                let children =
                    self.generate_children(&current_node, &query_case_stats, static_void_cost);
                for mut child in children {
                    if child.min_cost < global_upper_bound {
                        //child.action.log(self.model.clone());
                        open_list.push(child);
                    } else {
                        ////println!("Pruned node before exploring with min cost: {}", child.min_cost);
                    }
                }
            }

            open_list.sort_by(|a, b| a.min_cost.partial_cmp(&b.min_cost).unwrap());
            if (open_list.len() == 0 && best_node.is_none()) {
                println!("No solution found");
                current_node.partial_case_stats.pretty_print_stats();
            }
        }
        if (best_node.is_none()) {
            println!("No solution found after exploring {} nodes", counter);
        }
        println!("Total nodes explored: {}", counter);
        best_node
    }

    fn calculate_min_cost(
        &self,
        query_case_stats: &CaseStats,
        partial_case_stats: &CaseStats,
        static_cost: f64,
    ) -> f64 {
        let mut total_cost = 0.0;

        for (event_type, &partial_count) in &partial_case_stats.query_event_counts {
            let query_count = query_case_stats
                .query_event_counts
                .get(event_type)
                .unwrap_or(&0);
            total_cost += (partial_count as f64 - *query_count as f64).max(0.0);
        }

        for (object_type, &partial_count) in &partial_case_stats.query_object_counts {
            let query_count = query_case_stats
                .query_object_counts
                .get(object_type)
                .unwrap_or(&0);
            total_cost += (partial_count as f64 - *query_count as f64).max(0.0);
        }

        /*
        for (edge_type, &partial_count) in &partial_case_stats.query_edge_counts {
            let query_count = query_case_stats
                .query_edge_counts
                .get(edge_type)
                .unwrap_or(&0);
            total_cost += (partial_count as f64 - *query_count as f64).max(0.0);
        }*/

        for ((edge_type, a, b), &partial_count) in &partial_case_stats.edge_type_counts {
            let query_count = query_case_stats
                .edge_type_counts
                .get(&(*edge_type, *a, *b))
                .unwrap_or(&0);
            total_cost += (partial_count as f64 - *query_count as f64).max(0.0);
        }

        total_cost + static_cost
    }

    fn calculate_query_static_costs(
        &mut self,
        query_case: &CaseGraph,
        query_case_stats: CaseStats,
    ) -> f64 {
        let mut total_cost = 0.0;

        // for all events in the case but not in model transitions add cost of 1

        let mut unhittable_edges: HashSet<usize> = HashSet::new();

        for (event_type, &partial_count) in &query_case_stats.query_event_counts {
            if !self.model_transitions.contains(event_type) {
                total_cost += partial_count as f64;

                // get all node ids with this event type in the partial case
                query_case
                    .nodes
                    .values()
                    .filter(|node| match node {
                        Node::EventNode(event) => event.event_type.eq(event_type),
                        _ => false,
                    })
                    .for_each(|node| {
                        if let Some(adj) = query_case.adjacency.get(&node.id()) {
                            unhittable_edges.extend(adj.iter());
                        }
                    });
            }
        }

        total_cost + unhittable_edges.len() as f64
    }

    fn generate_children<'a>(
        &mut self,
        node: &SearchNode,
        query_case_stats: &CaseStats,
        static_cost: f64,
    ) -> Vec<SearchNode> {
        let mut children = Vec::new();

        // only add tokens to initial places, if the node is the initial node or follows a add token action node
        //println!("GEtting initial places");
        if (node.action.is_pre_firing()) {
            // a search node that is pre firing is dead, when it misses tokens in an initial place of higher lexicographical order in order to fire anything
            // this is because we can only add tokens to the initial places in lexicographical order

            // check this by checking [INSERT HERE]

            // Order the object types in the net lexicographically and remove duplicates
            let mut initial_place_names: Vec<String> = self
                .model
                .get_initial_places()
                .iter()
                .map(|p| p.object_type.clone())
                .collect::<HashSet<_>>() // Remove duplicates
                .into_iter()
                .collect();

            initial_place_names.sort(); // Sort lexicographically
                                        //initial_place_names.reverse();

            let mut initial_places = self.model.get_initial_places().clone();
            //initial_places = next_permutation(&initial_places);

            let counts_per_type = node.marking.get_initial_counts_per_type();

            // sort the place by amount of tokens in the marking
            initial_places.sort_by(|a, b| {
                let a_count = counts_per_type.get(&a.object_type).unwrap_or(&0);
                let b_count = counts_per_type.get(&b.object_type).unwrap_or(&0);

                let a_query_count = query_case_stats
                    .query_object_counts
                    .get(&a.object_type)
                    .unwrap_or(&0);
                let b_query_count = query_case_stats
                    .query_object_counts
                    .get(&b.object_type)
                    .unwrap_or(&0);

                let a_diff = (*a_query_count as i64 - *a_count as i64);
                let b_diff = (*b_query_count as i64 - *b_count as i64);

                if a_diff > 0 {
                    return b_diff.cmp(&a_diff);
                }

                if a_count == b_count {
                    if a_query_count == b_query_count {
                        return b.object_type.cmp(&a.object_type);
                    }
                    return b_query_count.cmp(&a_query_count);
                }
                b_count.cmp(&a_count)
            });

            for place in initial_places {
                // Find the index of the current object's type in the sorted list
                let type_index = initial_place_names
                    .iter()
                    .position(|t| t == &place.object_type)
                    .expect("Object type should exist in initial_places");

                // Check if any higher lexicographical types have been used (i.e., have a count > 0)
                let higher_types_used = initial_place_names[type_index + 1..]
                    .iter()
                    .any(|t| counts_per_type.get(t).map_or(false, |count| *count > 0));

                // Only allow incrementing if no higher types have been used
                if !higher_types_used {
                    // Proceed to add a token to this place
                    let mut new_marking = node.marking.clone();
                    let token_ids = new_marking.add_initial_token_count(&place.id, 1);

                    let mut new_partial_case = node.partial_case.clone();
                    let object_id = new_partial_case.get_new_id();

                    let new_object = Node::ObjectNode(Object {
                        id: object_id.clone(),
                        object_type: place.object_type.clone(),
                    });
                    new_partial_case.add_node(new_object);

                    let mut new_partial_case_stats = node.partial_case_stats.clone();
                    new_partial_case_stats
                        .query_object_counts
                        .entry(place.object_type.clone())
                        .and_modify(|e| *e += 1)
                        .or_insert(1);

                    let min_cost = self.calculate_min_cost(
                        &query_case_stats,
                        &new_partial_case_stats,
                        static_cost,
                    );
                    self.token_graph_id_mapping.insert(token_ids[0], object_id);

                    children.push(SearchNode::new_with_stats(
                        new_marking,
                        new_partial_case,
                        min_cost,
                        None,
                        SearchNodeAction::add_token(place.id.clone()),
                        new_partial_case_stats,
                    ));
                }
                // If higher_types_used is true, do not add tokens to this and lower types
                // Continue to the next place
            }
        }

        //println!("Getting transitions");

        //
        let mut transition_enabled: HashMap<Uuid, bool> = HashMap::new();

        let mut transition_children: Vec<SearchNode> = Vec::new();
        for transition in self.model.transitions.values() {
            //println!("Transition: {}", transition.name);
            let firing_combinations = node.marking.get_firing_combinations(transition);
            if (firing_combinations.len() > 0) {
                //println!("{}",transition.name);
                //println!("Firing combinations: {:?}", firing_combinations.iter().map(|c| c.to_string()).collect::<Vec<String>>());
            }

            transition_enabled.insert(transition.id, firing_combinations.len() > 0);

            firing_combinations.iter().for_each(|combination| {
                let mut new_marking = node.marking.clone();
                let mut new_partial_case = node.partial_case.clone();

                let mut most_recent_event_id = node.most_recent_event_id.clone();

                new_marking.fire_transition(transition, combination);
                let mut new_partial_case_stats = node.partial_case_stats.clone();

                if (!transition.silent) {
                    let event_id = new_partial_case.get_new_id();
                    let new_event = Node::EventNode(Event {
                        id: event_id,
                        event_type: transition.name.clone(),
                    });
                    new_partial_case.add_node(new_event);

                    let mut type_storage = TYPE_STORAGE.write().unwrap();

                    combination
                        .object_binding_info
                        .values()
                        .for_each(|binding_info| {
                            binding_info.tokens.iter().for_each(|token| {
                                let e20_edge_id = new_partial_case.get_new_id();
                                new_partial_case.add_edge(Edge::new(
                                    e20_edge_id,
                                    event_id,
                                    self.token_graph_id_mapping.get(&token.id).unwrap().clone(),
                                    EdgeType::E2O,
                                ));
                                new_partial_case_stats
                                    .query_edge_counts
                                    .entry(EdgeType::E2O)
                                    .and_modify(|e| *e += 1)
                                    .or_insert(1);

                                let a = type_storage.get_type_id(transition.name.as_str());
                                let b = type_storage.get_type_id(binding_info.object_type.as_str());

                                *new_partial_case_stats
                                    .edge_type_counts
                                    .entry((EdgeType::E2O, a, b))
                                    .or_insert(0) += 1;
                            })
                        });

                    let df_edge_id = new_partial_case.get_new_id();
                    if let Some(prev_event_id) = node.most_recent_event_id {
                        new_partial_case.add_edge(Edge::new(
                            df_edge_id,
                            prev_event_id,
                            event_id,
                            EdgeType::DF,
                        ));
                        new_partial_case_stats
                            .query_edge_counts
                            .entry(EdgeType::DF)
                            .and_modify(|e| *e += 1)
                            .or_insert(1);

                        let a = type_storage.get_type_id(
                            &new_partial_case
                                .get_node(prev_event_id)
                                .unwrap()
                                .type_name(),
                        );
                        let b = type_storage.get_type_id(transition.name.as_str());

                        *new_partial_case_stats
                            .edge_type_counts
                            .entry((EdgeType::DF, a, b))
                            .or_insert(0) += 1;
                    }

                    new_partial_case_stats
                        .query_event_counts
                        .entry(transition.name.clone())
                        .and_modify(|e| *e += 1)
                        .or_insert(1);

                    most_recent_event_id = Some(event_id);
                }

                let new_cost = self.calculate_min_cost(
                    &query_case_stats,
                    &new_partial_case_stats,
                    static_cost,
                );

                transition_children.push(SearchNode::new_with_stats(
                    new_marking,
                    new_partial_case,
                    new_cost,
                    most_recent_event_id,
                    SearchNodeAction::fire_transition(transition.id, combination.clone()),
                    new_partial_case_stats,
                ));
            });
        }
        //println!("done getting transitions");
        // places are allowed to be dead as long as we keep adding tokens to alive them ;)
        if (!node.action.is_pre_firing()) {
            if !node
                .marking
                .has_dead_places(&transition_enabled, &self.reachability_cache)
                .is_empty()
            {
                // print a list of the names of all dead places
                let dead_places = node
                    .marking
                    .has_dead_places(&transition_enabled, &self.reachability_cache);

                dead_places.iter().for_each(|place_id| {
                    println!(
                        "Dead place: {}",
                        self.model
                            .get_place(place_id)
                            .unwrap()
                            .name
                            .clone()
                            .unwrap_or("no_name".to_string())
                    );
                });

                return vec![];
            }
        }
        //println!("done checking dead");

        // sort the transitions by the difference between case stats and query case stats
        transition_children.sort_by(|a, b| {
            // prioritize transitions that have a higher difference between the case stats and the query case stats
            // all actions in this list are transitions
            let transition_a = a.action.transition_id().unwrap();

            let transition_name = &self.model.get_transition(&transition_a).unwrap().name;

            let difference_a = *query_case_stats
                .query_event_counts
                .get(transition_name)
                .unwrap_or(&0) as i64
                - *a.partial_case_stats
                    .query_event_counts
                    .get(transition_name)
                    .unwrap_or(&0) as i64;

            let transition_b = b.action.transition_id().unwrap();
            let transition_name = &self.model.get_transition(&transition_b).unwrap().name;

            let difference_b = *query_case_stats
                .query_event_counts
                .get(transition_name)
                .unwrap_or(&0) as i64
                - *b.partial_case_stats
                    .query_event_counts
                    .get(transition_name)
                    .unwrap_or(&0) as i64;

            difference_a.partial_cmp(&difference_b).unwrap()
        });
        return transition_children;
        //children.append(&mut transition_children);
        //children
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oc_align::visualization::case_visual::export_c2_with_alignment_image;
    use crate::oc_case::dummy_ocel_1_serialization::{
        json_to_case_graph, process_jsonocel_files, CaseGraphIterator,
    };
    use crate::oc_case::serialization::deserialize_case_graph;
    use crate::oc_case::visualization::export_case_graph_image;
    use crate::oc_petri_net::initialize_ocpn_from_json;
    use graphviz_rust::cmd::Format;
    use std::fs;
    use std::path::Path;

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
        let event1 = Node::EventNode(Event {
            id: 1,
            event_type: "T1".to_string(),
        });
        let event2 = Node::EventNode(Event {
            id: 2,
            event_type: "T2".to_string(),
        });
        query_case.add_node(event1);
        query_case.add_node(event2);

        // Connect the events with a direct follows edge
        query_case.add_edge(Edge::new(1, 1, 2, EdgeType::DF));

        // Initialize ModelCaseChecker
        let mut checker = ModelCaseChecker::new(petri_net_arc);

        // Use branch_and_bound to find alignment
        let result = checker.branch_and_bound(&query_case, initial_marking);

        // Validate if a result is found
        assert!(result.is_some(), "Failed to find a valid alignment");

        let best_node = result.unwrap();
        let total_cost = best_node.min_cost;

        // Print the results for debugging
        println!("Best alignment cost: {}", total_cost);
        //best_node.partial_case.print_mappings();
    }
    #[test]
    fn process_jsonocel_files_test() {
        let source_dir = "/Users/erikwrede/dev/uni/ma-py/ocgc-py/ocgc/vars";
        process_jsonocel_files(source_dir).expect("TODO: panic message");
    }
    #[test]
    fn other() {
        let json_data = fs::read_to_string("./src/oc_petri_net/util/oc_petri_net.json").unwrap();
        let ocpn = initialize_ocpn_from_json(&json_data);

        // Wrap the petri net in Arc to match branch_and_bound signature
        let petri_net_arc = Arc::new(ocpn);

        // deserialize a query case from a json file
        // load file as string
        let case_file = fs::read_to_string(
            "./src/oc_case/test_data/variant_6eb2da5f3f3f7ea94ca51f1a72de8f47.jsonocel",
        )
        .expect("Unable to read file");

        let mut query_case = json_to_case_graph(case_file.as_str());

        // now visualize the case graph
        export_case_graph_image(&query_case, "test_case_graph.png", Format::Png, Some(2.0))
            .unwrap();
    }

    #[test]
    fn large_petri_net() {
        let json_data =
            fs::read_to_string("./src/oc_align/test_data/bpi17/oc_petri_net.json").unwrap();
        let ocpn = initialize_ocpn_from_json(&json_data);

        let initial_marking = Marking::new(ocpn.clone());
        // Wrap the petri net in Arc to match branch_and_bound signature
        let petri_net_arc = Arc::new(ocpn);

        // deserialize a query case from a json file
        // load file as string

        let shortest_case_json =
            fs::read_to_string("./src/oc_align/test_data/bpi17/shortest_case_graph.json")
                .expect("Unable to read file");
        let shortest_case = deserialize_case_graph(shortest_case_json.as_str());

        // Initialize ModelCaseChecker
        let mut checker =
            ModelCaseChecker::new_with_shortest_case(petri_net_arc.clone(), shortest_case);

        let case_graph_iter =
            CaseGraphIterator::new("/Users/erikwrede/dev/uni/ma-py/ocgc-py/ocgc/varsbpi").unwrap();
        let visualized_dir =
            Path::new("/Users/erikwrede/dev/uni/ma-py/ocgc-py/ocgc/varsbpi_visualized");
        for (case_graph, path) in case_graph_iter {
            let output_file_name = path.file_stem().unwrap().to_str().unwrap().to_owned();
            let output_path = visualized_dir.join(output_file_name);

            export_case_graph_image(
                &case_graph,
                output_path.to_str().unwrap().to_owned() + "_query.png",
                Format::Png,
                Some(2.0),
            )
            .unwrap();

            let result = checker.branch_and_bound(&case_graph, initial_marking.clone());
            if let Some(result_node) = result {
                println!("Solution found for case {:?}", path);
                // save the alignment result as an image in a directory next to /Users/erikwrede/dev/uni/ma-py/ocgc-py/ocgc/varsbpi
                let alignment = CaseAlignment::align_mip(&case_graph, &result_node.partial_case);
                let cost = alignment.total_cost().unwrap_or(f64::INFINITY);
                export_c2_with_alignment_image(
                    &result_node.partial_case,
                    &alignment,
                    output_path.to_str().unwrap().to_owned() + format!("_aligned_cost_{}.png", cost).as_str(),
                    Format::Png,
                    Some(2.0),
                )
                .unwrap();

                export_case_graph_image(
                    &result_node.partial_case,
                    output_path.to_str().unwrap().to_owned() + "_target.png",
                    Format::Png,
                    Some(2.0),
                );
            } else {
                println!("No solution found for case {:?}", path);
            }
        }
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
        let event1 = Node::EventNode(Event {
            id: 1,
            event_type: "T1".to_string(),
        });
        let event2 = Node::EventNode(Event {
            id: 2,
            event_type: "T2".to_string(),
        });
        query_case.add_node(event1);
        query_case.add_node(event2);

        // Connect the events with a direct follows edge
        query_case.add_edge(Edge::new(2, 1, 2, EdgeType::DF));

        // Initialize ModelCaseChecker
        let mut checker = ModelCaseChecker::new(petri_net_arc);

        // Use branch_and_bound to find alignment
        let result = checker.branch_and_bound(&query_case, initial_marking);

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
