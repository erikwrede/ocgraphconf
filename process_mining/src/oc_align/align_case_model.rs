use crate::oc_align::align_case::EdgeMapping::{RealEdge, VoidEdge};
use crate::oc_align::align_case::NodeMapping::{RealNode, VoidNode};
use crate::oc_align::align_case::{CaseAlignment, EdgeMapping, Mappable, NodeMapping};
use crate::oc_align::util::reachability_cache::ReachabilityCache;
use crate::oc_align::util::shortest_path_cache::ShortestPathCache;
use crate::oc_align::visualization::case_visual::export_c2_with_alignment_image;
use crate::oc_case::case::{CaseGraph, CaseStats, Edge, EdgeType, Event, Node, Object};
use crate::oc_case::visualization::export_case_graph_image;
use crate::oc_petri_net::marking::{Binding, Marking, OCToken};
use crate::oc_petri_net::oc_petri_net::{ObjectCentricPetriNet, Transition};
use crate::type_storage::{EventType, ObjectType, TYPE_STORAGE};
use graphviz_rust::cmd::Format;
use peak_alloc::PeakAlloc;
use std::any::Any;
use std::cmp::{Ordering, PartialEq};
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::ops::{Add, Deref, Not};
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[global_allocator]
static PEAK_ALLOC: PeakAlloc = PeakAlloc;
#[derive(Debug, Clone)]
pub struct SearchNode {
    marking: Marking,
    pub partial_case: CaseGraph,
    pub min_cost: f64,
    pub static_min_cost: f64,
    // pub epsilon_cost: f64,
    // pub void_cost: f64,
    most_recent_event_id: Option<usize>,
    action_path: Vec<Arc<SearchNodeAction>>,
    forbidden_firings: Option<HashMap<Uuid, Vec<Arc<Binding>>>>,
    open_query_nodes: Vec<usize>, // FIXME dont use hashmap
    reverse_node_mapping: HashMap<usize, NodeMapping>, // cp_node_id -> query case mapping
    reverse_edge_mapping: HashMap<usize, EdgeMapping>, // cp_edge_id -> query case mapping

    depth: usize,
}

// Wrapper struct to establish min-heap ordering
struct OrderedSearchNode(SearchNode);

impl PartialEq for OrderedSearchNode {
    fn eq(&self, other: &Self) -> bool {
        self.0.min_cost == other.0.min_cost
    }
}

impl Eq for OrderedSearchNode {}

impl PartialOrd for OrderedSearchNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // Reverse ordering for min-heap
        other.0.min_cost.partial_cmp(&self.0.min_cost)
    }
}

impl Ord for OrderedSearchNode {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse ordering for min-heap
        other.0.min_cost.partial_cmp(&self.0.min_cost).unwrap()
    }
}

// Implement From<SearchNode> for OrderedSearchNode
impl From<SearchNode> for OrderedSearchNode {
    fn from(node: SearchNode) -> OrderedSearchNode {
        OrderedSearchNode(node)
    }
}

// Implement From<OrderedSearchNode> for SearchNode
impl From<OrderedSearchNode> for SearchNode {
    fn from(ordered_node: OrderedSearchNode) -> SearchNode {
        ordered_node.0
    }
}

impl SearchNode {
    fn new(
        marking: Marking,
        partial_case: CaseGraph,
        min_cost: f64,
        min_remaining_cost: f64,
        most_recent_event_id: Option<usize>,
        open_query_nodes: Vec<usize>,
        reverse_node_mapping: HashMap<usize, NodeMapping>,
        reverse_edge_mapping: HashMap<usize, EdgeMapping>,
        action: Vec<Arc<SearchNodeAction>>,
    ) -> Self {
        SearchNode {
            marking,
            //partial_case_stats: partial_case.get_case_stats(),
            partial_case,
            min_cost,
            static_min_cost: min_remaining_cost,
            most_recent_event_id,
            action_path: action,
            depth: 0,
            forbidden_firings: None,
            // epsilon_cost: 0.0,
            // void_cost: 0.0,
            open_query_nodes,
            reverse_node_mapping,
            reverse_edge_mapping,
        }
    }

    /// Manually pass the case stats, if you already know them from the changes of the previous node
    fn new_with_stats(
        marking: Marking,
        partial_case: CaseGraph,
        min_cost: f64,
        min_remaining_cost: f64,
        most_recent_event_id: Option<usize>,
        action: Vec<Arc<SearchNodeAction>>,
        partial_case_stats: Option<CaseStats>,
        depth: usize,
        open_query_nodes: Vec<usize>,
        reverse_node_mapping: HashMap<usize, NodeMapping>,
        reverse_edge_mapping: HashMap<usize, EdgeMapping>,
        forbidden_firings: Option<HashMap<Uuid, Vec<Arc<Binding>>>>,
        // epsilon_cost: f64,
        // void_cost: f64,
    ) -> Self {
        SearchNode {
            marking,
            //partial_case_stats,
            partial_case,
            min_cost,
            static_min_cost: min_remaining_cost,
            most_recent_event_id,
            action_path: action,
            depth,
            forbidden_firings,
            open_query_nodes,
            reverse_node_mapping,
            reverse_edge_mapping,
            // epsilon_cost,
            // void_cost,
        }
    }

    fn action_path(&self, model: Arc<ObjectCentricPetriNet>) -> String {
        self.action_path
            .iter()
            .filter_map(|action| match **action {
                SearchNodeAction::FireTransition(ref transition_id, ref binding) => {
                    let transition_name = model.get_transition(transition_id).unwrap().name.clone();
                    let binding_info: Vec<String> = binding
                        .object_binding_info
                        .iter()
                        .map(|(object_type, binding_info)| {
                            let tokens: Vec<String> = binding_info
                                .tokens
                                .iter()
                                .map(|token| token.id.to_string())
                                .collect();
                            format!("{}: [{}]", object_type.to_string(), tokens.join(", "))
                        })
                        .collect();
                    Some(format!("{} ({})", transition_name, binding_info.join(", ")))
                }
                _ => None,
            })
            .collect::<Vec<String>>()
            .join(" -> ")
    }
}

#[derive(Debug, Clone)]
enum SearchNodeAction {
    FireTransition(Uuid, Arc<Binding>),
    AddToken(Uuid),
    // used for the initial node
    VOID,
}

impl SearchNodeAction {
    fn fire_transition(transition_id: Uuid, binding: Arc<Binding>) -> Self {
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

pub struct ModelCaseChecker {
    token_graph_id_mapping: HashMap<usize, usize>,
    reachability_cache: ReachabilityCache,
    shortest_path_cache: ShortestPathCache,
    model: Arc<ObjectCentricPetriNet>,
    shortest_case: Option<CaseGraph>,
    model_transitions: HashSet<String>,
}
impl ModelCaseChecker {
    pub fn new(model: Arc<ObjectCentricPetriNet>) -> Self {
        ModelCaseChecker {
            token_graph_id_mapping: HashMap::new(),
            reachability_cache: ReachabilityCache::new(model.clone()),
            shortest_path_cache: ShortestPathCache::new(model.clone()),
            model_transitions: model.transitions.values().map(|t| t.name.clone()).collect(),
            model,
            shortest_case: None,
        }
    }

    pub fn new_with_shortest_case(
        model: Arc<ObjectCentricPetriNet>,
        shortest_case: CaseGraph,
    ) -> Self {
        ModelCaseChecker {
            token_graph_id_mapping: HashMap::new(),
            reachability_cache: ReachabilityCache::new(model.clone()),
            model_transitions: model.transitions.values().map(|t| t.name.clone()).collect(),
            shortest_path_cache: ShortestPathCache::new(model.clone()),
            model,
            shortest_case: Some(shortest_case),
        }
    }

    /// Recursive DFS branch&amp;bound with progress updates.
    fn dfs_branch_and_bound_progress(
        &mut self,
        node: &SearchNode,
        query_case: &CaseGraph,
        query_case_stats: &CaseStats,
        static_cost: f64,
        global_upper_bound: &mut f64,
        best_node: &mut Option<SearchNode>,
        counter: &mut usize,
        start_time: Instant,
        last_update: &mut Instant,
    ) {
        *counter += 1;
        if node.min_cost >= *global_upper_bound {
            return;
        }
        // Print progress update every 30 seconds.
        if last_update.elapsed() >= Duration::from_secs(30) {
            *last_update = Instant::now();
            println!("------------------ Progress Update ------------------");
            println!("Nodes explored: {}", counter);
            println!(
                "Elapsed time: {:.2} seconds",
                start_time.elapsed().as_secs_f64()
            );
            println!("Current node min cost: {}", node.min_cost);
            println!("Global upper bound: {}", global_upper_bound);
            let current_mem = PEAK_ALLOC.current_usage_as_mb();
            println!("This program currently uses {} MB of RAM.", current_mem);
            let peak_mem = PEAK_ALLOC.peak_usage_as_gb();
            println!("The max amount that was used {}", peak_mem);
            println!("------------------------------------------------------");
        }
        // Check if node is accepting and update upper bound if needed.
        if node.marking.is_final_has_tokens() {
            let alignment = CaseAlignment::align_mip(query_case, &node.partial_case);
            let cost = alignment.total_cost().unwrap();
            if cost < *global_upper_bound {
                *global_upper_bound = cost;
                *best_node = Some(node.clone());
                println!("New best bound found: {}", cost);
                println!("With min cost: {}", node.min_cost);
                println!("static share: {}", node.min_cost - node.static_min_cost);
                
                    export_c2_with_alignment_image(
                        &node.partial_case,
                        &alignment,
                        format!("./intermediates_2/alignment_{}_1case_cost_{}_{}.png", counter, node.min_cost, cost).as_str(),
                        Format::Png,
                        Some(2.0),
                    ).unwrap();
                    let void_nodes: HashMap<usize, Node> = node
                        .reverse_node_mapping
                        .iter()
                        .filter_map(|(id, mapping)| {
                            if let VoidNode(_, _void_id) = mapping {
                                // Assuming get_node returns a reference and you wish to clone it
                                node.partial_case.get_node(*id).cloned().map(|node| (*id, node))
                            } else {
                                None
                            }
                        })
                        .collect();
                    
                    let void_edges: HashMap<usize, Edge> = node
                        .reverse_edge_mapping
                        .iter()
                        .filter_map(|(id, mapping)| {
                            if let VoidEdge(_, _void_id) = mapping {
                                // Assuming get_node returns a reference and you wish to clone it
                                node.partial_case.get_edge(*id).cloned().map(|edge| (*id, edge))
                            } else {
                                None
                            }
                        })
                        .collect();
                    let alignment = CaseAlignment {
                        c1: &node.partial_case,
                        c2: query_case,
                        void_nodes,
                        void_edges,
                        edge_mapping: node.reverse_edge_mapping.clone(),
                        node_mapping: node.reverse_node_mapping.clone(),
                    };
                
                if(alignment.total_cost().unwrap() < node.min_cost) {
                    println!("New best bound found: {}", cost);
                    println!("With min cost: {}", node.min_cost);
                    println!("static share: {}", node.min_cost - node.static_min_cost);
                    println!("Epsilon cost: {}", node.epsilon_cost);
                    println!("Void cost: {}", node.void_cost);
                    panic!("MinCost incorrect")
                }
                    println!("Actual synchronous alignment cost: {}", alignment.total_cost().unwrap_or(f64::INFINITY));
                    // print a picture of the alignment
                    export_c2_with_alignment_image(
                        &query_case,
                        &alignment,
                        format!("./intermediates_2/alignment_{}_2case_cost_{}_{}.png", counter, node.min_cost, cost).as_str(),
                        Format::Png,
                        Some(2.0),
                    ).unwrap();
                    // print a picture of the alignment
                    export_c2_with_alignment_image(
                        &node.partial_case,
                        &alignment.reverse(),
                        format!("./intermediates_2/alignment_{}_3case_cost_{}_{}.png", counter, node.min_cost, cost).as_str(),
                        Format::Png,
                        Some(2.0),
                    ).unwrap();
            }
            // if cost < node.min_cost {
            //     println!("===================");
            //     println!("Solution: {cost}, MinCost: {}", node.min_cost);
            //     println!("Static min cost: {}", node.static_min_cost);
            //     println!("Epsilon cost: {}", node.epsilon_cost);
            //     println!("Void cost: {}", node.void_cost);
            //     println!("!!!!This is a better bound than the node min cost");
            //     println!("Node min remaining cost: {}", node.min_cost - node.static_min_cost);
            //     println!("===================");
            //     
            //     // print a picture of the alignment
            //     export_c2_with_alignment_image(
            //         &node.partial_case,
            //         &alignment,
            //         format!("./intermediates_2/alignment_{}_case_cost_{}_{}.png", counter, node.min_cost, cost).as_str(),
            //         Format::Png,
            //         Some(2.0),
            //     ).unwrap();
            //     let void_nodes: HashMap<usize, Node> = node
            //         .reverse_node_mapping
            //         .iter()
            //         .filter_map(|(id, mapping)| {
            //             if let VoidNode(_, _void_id) = mapping {
            //                 // Assuming get_node returns a reference and you wish to clone it
            //                 node.partial_case.get_node(*id).cloned().map(|node| (*id, node))
            //             } else {
            //                 None
            //             }
            //         })
            //         .collect();
            //     
            //     let void_edges: HashMap<usize, Edge> = node
            //         .reverse_edge_mapping
            //         .iter()
            //         .filter_map(|(id, mapping)| {
            //             if let VoidEdge(_, _void_id) = mapping {
            //                 // Assuming get_node returns a reference and you wish to clone it
            //                 node.partial_case.get_edge(*id).cloned().map(|edge| (*id, edge))
            //             } else {
            //                 None
            //             }
            //         })
            //         .collect();
            //     let alignment = CaseAlignment {
            //         c1: &node.partial_case,
            //         c2: query_case,
            //         void_nodes,
            //         void_edges,
            //         edge_mapping: node.reverse_edge_mapping.clone(),
            //         node_mapping: node.reverse_node_mapping.clone(),
            //     };
            //     println!("Actual synchronous alignment cost: {}", alignment.total_cost().unwrap_or(f64::INFINITY));
            //     // print a picture of the alignment
            //     export_c2_with_alignment_image(
            //         &query_case,
            //         &alignment,
            //         format!("./intermediates_2/alignment_{}_2case_cost_{}_{}.png", counter, node.min_cost, cost).as_str(),
            //         Format::Png,
            //         Some(2.0),
            //     ).unwrap();
            //     // print a picture of the alignment
            //     export_c2_with_alignment_image(
            //         &node.partial_case,
            //         &alignment.reverse(),
            //         format!("./intermediates_2/alignment_{}_3case_cost_{}_{}.png", counter, node.min_cost, cost).as_str(),
            //         Format::Png,
            //         Some(2.0),
            //     ).unwrap();
            // }
        }
        // Generate and sort children by increasing min_cost.
        let mut children = self.generate_children(node, query_case_stats, query_case, static_cost);
        children.sort_by(|a, b| a.min_cost.partial_cmp(&b.min_cost).unwrap());
        // Explore children if promising.
        for child in children {
            if child.min_cost < *global_upper_bound {
                self.dfs_branch_and_bound_progress(
                    &child,
                    query_case,
                    query_case_stats,
                    static_cost,
                    global_upper_bound,
                    best_node,
                    counter,
                    start_time,
                    last_update,
                );
            }
        }
    }

    /// bnb_v2_progress initializes DFS branch&amp;bound DFS with progress updates.
    pub fn bnb_v2(
        &mut self,
        query_case: &CaseGraph,
        initial_marking: Marking,
    ) -> Option<SearchNode> {
        let query_case_stats = query_case.get_case_stats();
        query_case_stats.pretty_print_stats();
        let static_cost = self.calculate_query_static_costs(query_case, &query_case_stats);

        // Initialize root node.
        let root_node =
            self.initialize_node_with_initial_places(query_case, initial_marking, static_cost);
        let mut best_node: Option<SearchNode> = None;
        let mut global_upper_bound = if let Some(shortest_case) = &self.shortest_case {
            let alignment = CaseAlignment::align_mip(query_case, shortest_case);
            alignment.total_cost().unwrap_or(f64::INFINITY)
        } else {
            f64::INFINITY
        };

        let mut counter = 0;
        let start_time = Instant::now();
        let mut last_update = Instant::now();

        self.dfs_branch_and_bound_progress(
            &root_node,
            query_case,
            &query_case_stats,
            static_cost,
            &mut global_upper_bound,
            &mut best_node,
            &mut counter,
            start_time,
            &mut last_update,
        );
        println!("Total nodes explored (DFS branch-and-bound): {}", counter);
        best_node
    }

    fn initialize_node_with_initial_places(
        &mut self,
        query_case: &CaseGraph,
        mut new_marking: Marking,
        static_cost: f64,
    ) -> SearchNode {
        let mut new_partial_case = CaseGraph::new();
        let mut reverse_node_mapping: HashMap<usize, NodeMapping> = HashMap::new();

        // Get the initial places from the model
        let initial_places: HashSet<ObjectType> = self
            .model
            .get_initial_places()
            .iter()
            .map(|p| p.oc_object_type.clone())
            .collect();

        // Initialize marking and case from query_case_stats, only for initial places
        &query_case
            .nodes
            .values()
            .filter(|node| node.is_object())
            .for_each(|node| {
                let object_type: ObjectType = node.oc_type_id().into();
                if initial_places.contains(&object_type) {
                    // Find the initial place with the same object type
                    // TODO support multiple initial places
                    let place = self
                        .model
                        .get_initial_places()
                        .iter()
                        .find(|p| &p.oc_object_type == &object_type)
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
                    reverse_node_mapping
                        .insert(new_object.id(), RealNode(new_object.id(), node.id()));
                    new_partial_case.add_node(new_object);

                    // Map the token to object id
                    self.token_graph_id_mapping.insert(token_ids[0], object_id);
                }
            });
        let open_node_list: Vec<_> = query_case
            .nodes
            .values()
            .filter(|node| node.is_event())
            .map(|node| node.id())
            .collect();
        println!("Open node list length {}", open_node_list.len());
        // Create and return the initialized SearchNode
        SearchNode::new(
            new_marking,
            new_partial_case,
            0 as f64,
            0.0,
            None,
            query_case
                .nodes
                .values()
                .filter(|node| node.is_event())
                .map(|node| node.id())
                .collect(),
            reverse_node_mapping,
            HashMap::new(),
            vec![Arc::new(SearchNodeAction::VOID)],
        )
    }

    pub fn branch_and_bound<'a>(
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

        let query_case_stats = query_case.get_case_stats();

        query_case_stats.pretty_print_stats();
        let static_void_cost = self.calculate_query_static_costs(query_case, &query_case_stats);

        let mut any_solution_found = false;

        if let Some(shortest_case) = &self.shortest_case {
            println!("Calculating initial upper bound");
            let alignment = CaseAlignment::align_mip(query_case, shortest_case);
            global_upper_bound = alignment.total_cost().unwrap_or(f64::INFINITY);
            println!("Initial upper bound: {}", global_upper_bound);
            // FIXME hier wieder richtig initialisieren
            best_node = Some(SearchNode::new(
                initial_marking.clone(),
                shortest_case.clone(),
                global_upper_bound,
                global_upper_bound,
                None,
                vec![],
                HashMap::new(),
                HashMap::new(),
                vec![Arc::new(SearchNodeAction::VOID)],
            ));
        }

        let mut open_list: BinaryHeap<OrderedSearchNode> = BinaryHeap::with_capacity(20_000_000);

        open_list.push(
            self.initialize_node_with_initial_places(
                &query_case,
                initial_marking.clone(),
                static_void_cost,
            )
            .into(),
        );

        // open_list.push(
        //     SearchNode::new(
        //         initial_marking.clone(),
        //         CaseGraph::new(),
        //         0.0,
        //         None,
        //         vec![Arc::new(SearchNodeAction::VOID)],
        //     )
        //     .into(),
        // );
        // let mut open_list: Vec<SearchNode> = vec![
        //     /*SearchNode::new(
        //         initial_marking,
        //         CaseGraph::new(),
        //         0.0,
        //         None,
        //         SearchNodeAction::VOID,
        //     )*/
        //     self.initialize_node_with_initial_places(
        //         &query_case_stats,
        //         initial_marking.clone(),
        //         static_void_cost,
        //     ),
        // ];
        //println!("starting");
        let mut counter = 0;
        let mut mip_counter = 0;
        // save current time
        let mut most_recent_timestamp = std::time::Instant::now();
        let beginning_timestamp = most_recent_timestamp;
        while let Some(current_node) = open_list.pop() {
            let current_node: SearchNode = current_node.into();

            if current_node.min_cost >= global_upper_bound {
                break;
            }
            if (global_lower_bound > current_node.min_cost) {
                println!("Old lb {}", global_lower_bound);
                println!("New lb {}", current_node.min_cost);
                println!(
                    "share of heuristic {}",
                    current_node.min_cost - current_node.static_min_cost
                );
                panic!("Lower bound decreased wtf!")
            }
            global_lower_bound = current_node.min_cost;

            counter += 1;
            // every 5 seconds print an update
            if most_recent_timestamp.elapsed().as_secs() >= 30 {
                most_recent_timestamp = std::time::Instant::now();
                println!("===================== Progress update =====================");
                println!("Nodes explored: {}", counter);
                println!(
                    "Exploration rate: {} nodes per second",
                    counter as f64 / beginning_timestamp.elapsed().as_secs_f64()
                );
                println!("Nodes aligned: {}", mip_counter);
                println!("Open list length: {}", open_list.len());
                println!("Current node min cost: {}", current_node.min_cost);
                println!("Global Upper Bound: {}", global_upper_bound);
                let current_mem = PEAK_ALLOC.current_usage_as_mb();
                println!("This program currently uses {} MB of RAM.", current_mem);
                let peak_mem = PEAK_ALLOC.peak_usage_as_gb();
                println!("The max amount that was used {}", peak_mem);
                println!("---------------------");
                // please output a histogram of costs in the open list
                let mut histogram: HashMap<usize, usize> = HashMap::new();
                for node in &open_list {
                    let cost = node.0.min_cost as usize;
                    *histogram.entry(cost).or_insert(0) += 1;
                }
                let mut histogram_vec: Vec<(usize, usize)> = histogram.into_iter().collect();
                histogram_vec.sort_by(|a, b| a.0.cmp(&b.0));
                println!("Histogram of costs in open list:");
                for (cost, count) in histogram_vec {
                    println!("Cost: {} Count: {}", cost, count);
                }
                // println!("Current node partial case stats:");
                // current_node.partial_case_stats.pretty_print_stats();
                // println!("---------------------");
                // println!("Query Case Stats:");
                // query_case_stats.pretty_print_stats();
                // println!("---------------------");
                // println!("Stats difference");
                // // for each hashmap entry print the difference like in calculate_min_cost
                // for (event_type, &partial_count) in
                //     &current_node.partial_case_stats.query_event_counts
                // {
                //     let query_count = query_case_stats
                //         .query_event_counts
                //         .get(event_type)
                //         .unwrap_or(&0);
                //     println!(
                //         "Event: {} Difference: {}",
                //         event_type,
                //         (partial_count as f64 - *query_count as f64)
                //     );
                // }
                // // for each edge do the same
                // for ((edge_type, a, b), &partial_count) in
                //     &current_node.partial_case_stats.edge_type_counts
                // {
                //     let query_count = query_case_stats
                //         .edge_type_counts
                //         .get(&(*edge_type, *a, *b))
                //         .unwrap_or(&0);
                //     let type_storage = TYPE_STORAGE.read().unwrap();
                //     println!(
                //         "Edge: ({:?},{},{}) Difference: {}",
                //         edge_type,
                //         type_storage.get_type_name(*a).unwrap(),
                //         type_storage.get_type_name(*b).unwrap(),
                //         (partial_count as f64 - *query_count as f64)
                //     );
                // }
                //
                // // print number of tokens in initial places of the current node marking
                // let initial_place_names: Vec<String> = self
                //     .model
                //     .get_initial_places()
                //     .iter()
                //     .map(|p| p.object_type.clone())
                //     .collect::<HashSet<_>>() // Remove duplicates
                //     .into_iter()
                //     .collect();
                // let counts = current_node.marking.get_initial_counts_per_type();
                // println!("Tokens in initial places: {:?}", counts);
                println!("depth: {}", current_node.depth);
                println!(
                    "open node list length: {}",
                    current_node.open_query_nodes.len()
                );

                // save an intermediate result as an image in ./intermediates
                let intermediate_graph = current_node.partial_case.clone();
                let intermediate_alignment =
                    CaseAlignment::align_mip(query_case, &intermediate_graph);
                println!(
                    "Intermediate alignment cost: {}",
                    intermediate_alignment.total_cost().unwrap_or(f64::INFINITY)
                );
                // export_case_graph_image(
                //     &intermediate_graph,
                //     format!("./intermediates_4/intermediate_{}_case.png", counter).as_str(),
                //     Format::Png,
                //     Some(0.75),
                // )
                // .unwrap();
                //
                // export_c2_with_alignment_image(
                //     &intermediate_graph,
                //     &intermediate_alignment,
                //     format!("./intermediates_4/intermediate_{}_alignment.png", counter).as_str(),
                //     Format::Png,
                //     Some(2.0),
                // )
                // .unwrap();
            }
            if (counter >= 800000) && false {
                println!("No solution found");
                //current_node.partial_case_stats.pretty_print_stats();
                //let events = current_node.partial_case_stats.query_event_counts.keys();
                //println!("Events: {:?}", events);
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
            //let events = current_node.partial_case_stats.query_event_counts.keys();
            //println!("Events: {:?}", events);

            let generate_children = true;
            if current_node.marking.is_final_has_tokens() {
                mip_counter += 1;
                if (!any_solution_found) {
                    any_solution_found = true;
                    println!("First final marking reached");
                    println!("depth: {}", current_node.depth);
                }
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
                //ensure dir exists
                // std::fs::create_dir_all("./alignments").unwrap();
                // export_c2_with_alignment_image(
                //     &current_node.partial_case,
                //     &alignment,
                //     format!("./alignments/alignment_{}_cost_{}_min_{}.png", counter, alignment_cost, current_node.min_cost).as_str(),
                //     Format::Png,
                //     Some(2.0),
                // ).unwrap();
                //
                // // print the action path
                // println!("Action path for count {}: {}", counter, current_node.action_path(self.model.clone()));

                // println!("Final marking reached after exploring {} nodes", counter);
                // println!("Cost: {}", alignment_cost);
                // println!(
                //     "fired events: {:?}",
                //     current_node.partial_case_stats.query_event_counts
                // );
                //panic!("Final marking reached");
                // Limit the scope of the mutable borrow using a separate block

                if alignment_cost < global_upper_bound {
                    // log that new best bound has been found
                    println!("New best bound found: {}", alignment_cost);
                    println!("Compared to node min cost: {}", current_node.min_cost);
                    if(alignment_cost < current_node.min_cost) {
                        println!("!!!!This is a better bound than the node min cost");
                        println!("Node min remaining cost: {}", current_node.min_cost - current_node.static_min_cost);
                    } 

                    global_upper_bound = alignment_cost;
                    best_node = Some(current_node.clone());
                    let len = open_list
                        .iter()
                        .filter(|node| node.0.min_cost >= global_upper_bound)
                        .count();

                    // println!("Number of nodes pruned due to best bound: {}", len);

                    // remove all nodes that have a cost higher than the new upper bound
                    //open_list.retain(|node| node.0.min_cost < global_upper_bound);
                }
            }

            if (generate_children) {
                let children = self.generate_children(
                    &current_node,
                    &query_case_stats,
                    &query_case,
                    static_void_cost,
                );
                for mut child in children {
                    if child.min_cost < global_upper_bound {
                        //child.action.log(self.model.clone());
                        open_list.push(child.into());
                    } else {
                        ////println!("Pruned node before exploring with min cost: {}", child.min_cost);
                    }
                }
            }

            //open_list.sort_by(|a, b| b.min_cost.partial_cmp(&a.min_cost).unwrap());
            if (open_list.len() == 0 && best_node.is_none()) {
                println!("No solution found");
                // current_node.partial_case_stats.pretty_print_stats();
            }
        }
        if (best_node.is_none()) {
            println!("No solution found after exploring {} nodes", counter);
        }
        println!("Total nodes explored: {}", counter);
        best_node
    }

    fn calculate_unreachable_events(
        &self,
        marking: &Marking,
        open_query_nodes: &Vec<usize>,
        query_case: &CaseGraph,
    ) -> usize {
        let cost_list: Vec<usize> = open_query_nodes
            .iter()
            .map(|node_id| {
                let node: &Node = query_case.nodes.get(node_id).unwrap();
                let node_cost = query_case.adjacency.get(node_id).unwrap().len() + 1;
                let node_cost = query_case
                    .adjacency
                    .get(node_id)
                    .unwrap()
                    .iter()
                    .map(|edge| query_case.edges.get(edge).unwrap())
                    // this is TO 
                    // TODO add cost for df edges between two unreachable void nodes
                    .filter(|edge| !(edge.edge_type == EdgeType::DF && edge.from == node.id()))
                    .count() + 1;
                let node_transition = self
                    .model
                    .transitions
                    .values()
                    .find(|t| t.event_type.0 == node.oc_type_id());
                if (node_transition.is_none()) {
                    println!(
                        "Couldnt find transition for event type: {}",
                        node.type_name()
                    );
                    return node_cost;
                }
                // get all input places to b and their object types
                let input_places = node_transition
                    .unwrap()
                    .input_arcs
                    .iter()
                    .map(|arc| arc.source_place_id)
                    .collect::<Vec<_>>();
                let mut b_input_object_types = input_places
                    .iter()
                    .map(|place_id| {
                        (
                            (place_id.clone()/*,
                            self.model
                                .get_place(place_id)
                                .unwrap()
                                .oc_object_type
                                .clone(),*/),
                            false,
                        )
                    })
                    .collect::<HashMap<_, _>>();

                // check b reachable from any place with a token using reachability cache
                marking
                    .assignments
                    .iter()
                    .filter(|(place_id, tokens)| tokens.len() > 0)
                    .for_each(|(place_id, tokens)| {
                        for (place_id_b) in &input_places {
                            if self.reachability_cache.is_reachable(place_id, place_id_b) {
                                b_input_object_types.insert(place_id_b.clone(), true);
                            }
                        }
                    });
                let all_reachable = b_input_object_types.values().all(|v| *v);

                if (!all_reachable) {
                    return node_cost;
                }
                return 0;
            })
            .filter(|cost| cost > &0)
            .collect();
        let cost: usize = cost_list.iter().sum();
        if (cost > 0) {
            //println!("Successfully added {} remaining cost", cost)
        }
        let mut more_epsilon_cost = 0;
        if (cost_list.len() == open_query_nodes.len()) {
            // remaining cost
            //println!("Adding remaining cost!");
            marking
                .assignments
                .iter()
                .filter(|(place_id, _)| {
                    let place = self.model.get_place(place_id).unwrap();
                    !place.final_place
                })
                .for_each(|(place_id, count)| {
                    let place = self.model.get_place(place_id).unwrap();

                    let final_place = self
                        .model
                        .get_final_place_for_type(&place.object_type)
                        .unwrap();

                    let shortest_path = self
                        .shortest_path_cache
                        .shortest_path(&place.id, &final_place.id)
                        .unwrap()
                        .distance;
                    more_epsilon_cost += (shortest_path * count.len());
                });
            //
        }

        return cost + more_epsilon_cost;
    }

    fn calculate_unreachable_events_verbose(
        &self,
        marking: &Marking,
        open_query_nodes: &Vec<usize>,
        query_case: &CaseGraph,
    ) -> usize {
        let cost_list: Vec<usize> = open_query_nodes
            .iter()
            .map(|node_id| {
                let node: &Node = query_case.nodes.get(node_id).unwrap();
                let node_cost = query_case.adjacency.get(node_id).unwrap().len() + 1;
                let node_transition = self
                    .model
                    .transitions
                    .values()
                    .find(|t| t.event_type.0 == node.oc_type_id());
                if (node_transition.is_none()) {
                    println!(
                        "Couldnt find transition for event type: {}",
                        node.type_name()
                    );
                    return node_cost;
                }
                // get all input places to b and their object types
                let input_places = node_transition
                    .unwrap()
                    .input_arcs
                    .iter()
                    .map(|arc| arc.source_place_id)
                    .collect::<Vec<_>>();
                let mut b_input_object_types = input_places
                    .iter()
                    .map(|place_id| {
                        (
                            (place_id.clone()/*,
                            self.model
                                .get_place(place_id)
                                .unwrap()
                                .oc_object_type
                                .clone(),*/),
                            false,
                        )
                    })
                    .collect::<HashMap<_, _>>();

                // check b reachable from any place with a token using reachability cache
                marking
                    .assignments
                    .iter()
                    .filter(|(place_id, tokens)| tokens.len() > 0)
                    .for_each(|(place_id, tokens)| {
                        for (place_id_b) in &input_places {
                            if self.reachability_cache.is_reachable(place_id, place_id_b) {
                                b_input_object_types.insert(place_id_b.clone(), true);
                            }
                        }
                    });
                let all_reachable = b_input_object_types.values().all(|v| *v);

                if (!all_reachable) {
                    println!(
                        "not all reachable for transition {}",
                        node_transition.unwrap().name
                    );
                    println!(
                        "not all reachable for transition {}",
                        node_transition.unwrap().name
                    );
                    println!(
                        "not all reachable for transition {}",
                        node_transition.unwrap().name
                    );
                    println!(
                        "not all reachable for transition {}",
                        node_transition.unwrap().name
                    );
                    println!(
                        "not all reachable for transition {}",
                        node_transition.unwrap().name
                    );
                    return node_cost;
                }
                return 0;
            })
            .filter(|cost| cost > &0)
            .collect();
        let cost = cost_list.iter().sum();
        if (cost > 0) {
            println!("Successfully added {} remaining cost", cost)
        }

        if (cost_list.len() == open_query_nodes.len()) {
            // remaining cost
        }
        println!("i was called with cost!");

        cost
    }

    #[inline(never)]
    fn calculate_min_cost(
        &self,
        query_case_stats: &CaseStats,
        partial_case_stats: &CaseStats,
        static_cost: f64,
        marking: &Marking,
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

        let mut more_epsilon_than_void = true;

        for ((edge_type, a, b), &query_count) in &query_case_stats.edge_type_counts {
            let partial_count = partial_case_stats
                .edge_type_counts
                .get(&(*edge_type, *a, *b))
                .unwrap_or(&0);

            if (edge_type.eq(&EdgeType::E2O) && *partial_count == 0) {
                let type_storage = TYPE_STORAGE.read().unwrap();
                let type_name = type_storage.get_type_name(*a).unwrap();
                if (self.model_transitions.contains(&type_name.to_string())) {
                    more_epsilon_than_void = false;
                    break;
                } else {
                    println!("false!!!!!")
                }
            } else if (*partial_count < query_count && edge_type.eq(&EdgeType::E2O)) {
                more_epsilon_than_void = false;
                break;
            }
        }

        for ((edge_type, a, b), &partial_count) in &partial_case_stats.edge_type_counts {
            let query_count = query_case_stats
                .edge_type_counts
                .get(&(*edge_type, *a, *b))
                .unwrap_or(&0);
            total_cost += (partial_count as f64 - *query_count as f64).max(0.0);
        }

        let mut more_epsilon_cost = 0.0;
        if (more_epsilon_than_void) {
            marking
                .assignments
                .iter()
                .filter(|(place_id, _)| {
                    let place = self.model.get_place(place_id).unwrap();
                    !place.final_place
                })
                .for_each(|(place_id, count)| {
                    let place = self.model.get_place(place_id).unwrap();

                    let final_place = self
                        .model
                        .get_final_place_for_type(&place.object_type)
                        .unwrap();

                    let shortest_path = self
                        .shortest_path_cache
                        .shortest_path(&place.id, &final_place.id)
                        .unwrap()
                        .distance;
                    more_epsilon_cost += (shortest_path * count.len()) as f64;
                });
            //println!("More_epsilon_cost: {}", more_epsilon_cost)
        }

        let mut added_void_cost = 0.0;
        let mut unreachable_events: HashSet<EventType> = HashSet::new();
        // print a list of edges in query_case_stats where query_count > partial_count
        for ((edge_type, a, b), &query_count) in &query_case_stats.edge_type_counts {
            if (*edge_type == EdgeType::DF) {
                let partial_count = partial_case_stats
                    .edge_type_counts
                    .get(&(*edge_type, *a, *b))
                    .unwrap_or(&0);
                let difference: isize = query_count as isize - *partial_count as isize;
                if (difference > 0) {
                    let b_type: EventType = b.clone().into();

                    let b_transition = self
                        .model
                        .transitions
                        .values()
                        .find(|t| t.event_type == b_type);
                    if (b_transition.is_none()) {
                        println!(
                            "Couldnt find transition for event type: {}",
                            b_type.to_string()
                        );
                        added_void_cost += difference as f64;
                        unreachable_events.insert(b_type);
                        continue;
                    }
                    // get all input places to b and their object types
                    let b_input_places = b_transition
                        .unwrap()
                        .input_arcs
                        .iter()
                        .map(|arc| arc.source_place_id)
                        .collect::<Vec<_>>();
                    let mut b_input_object_types = b_input_places
                        .iter()
                        .map(|place_id| {
                            (
                                (place_id.clone()/*,
                                self.model
                                    .get_place(place_id)
                                    .unwrap()
                                    .oc_object_type
                                    .clone(),*/),
                                false,
                            )
                        })
                        .collect::<HashMap<_, _>>();

                    // check b reachable from any place with a token using reachability cache

                    marking
                        .assignments
                        .iter()
                        .filter(|(place_id, tokens)| tokens.len() > 0)
                        .for_each(|(place_id, tokens)| {
                            for (place_id_b) in &b_input_places {
                                if self.reachability_cache.is_reachable(place_id, place_id_b) {
                                    b_input_object_types.insert(place_id_b.clone(), true);
                                }
                            }
                        });
                    let all_reachable = b_input_object_types.values().all(|v| *v);

                    if (!all_reachable) {
                        added_void_cost += difference as f64;
                        unreachable_events.insert(b_type);
                    }
                }
            }
        }

        let mut added_void_event_cost = 0.0;
        if (!unreachable_events.is_empty()) {
            for ((edge_type, a, b), &query_count) in &query_case_stats.edge_type_counts {
                if (*edge_type == EdgeType::E2O && unreachable_events.contains(&a.clone().into())) {
                    let partial_count = partial_case_stats
                        .edge_type_counts
                        .get(&(*edge_type, *a, *b))
                        .unwrap_or(&0);
                    let difference: isize = query_count as isize - *partial_count as isize;
                    if (difference > 0) {
                        added_void_event_cost += difference as f64;
                    }
                }
            }
        }

        //if (added_void_event_cost > 0.0) {
        //    println!("Added void cost: {}", added_void_event_cost);
        //}

        total_cost + static_cost + more_epsilon_cost + added_void_cost + added_void_event_cost
    }

    fn more_epsilon_than_void_with_debug(&self, marking: &Marking) {
        let mut more_epsilon_cost = 0.0;
        marking
            .assignments
            .iter()
            .filter(|(place_id, _)| {
                let place = self.model.get_place(place_id).unwrap();
                !place.final_place
            })
            .for_each(|(place_id, count)| {
                let place = self.model.get_place(place_id).unwrap();

                let final_place = self
                    .model
                    .get_final_place_for_type(&place.object_type)
                    .unwrap();

                let shortest_path = self
                    .shortest_path_cache
                    .shortest_path(&place.id, &final_place.id)
                    .unwrap()
                    .distance;

                if (count.len() > 0) {
                    println!(
                        "Adding {} * {} = {} for path between {} {}",
                        shortest_path,
                        count.len(),
                        (shortest_path * count.len()) as f64,
                        place.name.clone().unwrap_or("".to_string()),
                        final_place.name.clone().unwrap_or("".to_string())
                    );
                }
                more_epsilon_cost += (shortest_path * count.len()) as f64;
            });
        //println!("More_epsilon_cost: {}", more_epsilon_cost)
    }

    fn calculate_query_static_costs(
        &mut self,
        query_case: &CaseGraph,
        query_case_stats: &CaseStats,
    ) -> f64 {
        let mut total_cost = 0.0;

        // for all events in the case but not in model transitions add cost of 1

        let mut unhittable_edges: HashSet<usize> = HashSet::new();

        for (event_type, &partial_count) in &query_case_stats.query_event_counts {
            let evtypename = event_type.to_string();
            if !self.model_transitions.contains(&evtypename) {
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
    #[inline(never)]
    fn filter_firing_combinations(
        &self,
        firing_combinations: &Vec<Arc<Binding>>,
        transition: &Transition,
        node: &SearchNode,
    ) -> Vec<Arc<Binding>> {
        // If the transition has variable arcs, retain all combinations
        if transition.input_arcs.iter().any(|arc| arc.variable) {
            return firing_combinations.iter().cloned().collect();
        }

        // Map of object_type to its sorted list of unused tokens
        let mut unused_objects_per_type: HashMap<ObjectType, Vec<&OCToken>> = HashMap::new();

        // Determine all object types involved in this transition
        // Assuming a method `get_object_types_for_transition` exists

        let object_types = firing_combinations
            .get(0)
            .unwrap()
            .object_binding_info
            .keys()
            .collect::<Vec<_>>();

        for object_type in object_types {
            // Retrieve all tokens of this object type from all bindings
            let all_tokens: HashSet<&OCToken> = firing_combinations
                .iter()
                .flat_map(|binding| {
                    binding
                        .object_binding_info
                        .get(object_type)
                        .map_or(Vec::new(), |binding_info| {
                            binding_info.tokens.iter().collect()
                        })
                })
                .collect();

            // Determine which tokens are already used (have adjacent edges)
            let used_tokens: HashSet<&OCToken> = all_tokens
                .iter()
                .filter_map(|token| {
                    let obj_id = self.token_graph_id_mapping.get(&token.id).unwrap();
                    let adj_edges = node.partial_case.adjacency.get(obj_id);

                    match adj_edges {
                        Some(edges) => {
                            if edges.is_empty() {
                                None
                            } else {
                                Some(*token)
                            }
                        }
                        None => None,
                    }
                })
                .collect();

            // Identify unused tokens
            let unused_tokens: Vec<&OCToken> = all_tokens
                .iter()
                .filter_map(|token| {
                    if !used_tokens.contains(*token) {
                        Some(*token)
                    } else {
                        None
                    }
                })
                .collect();

            // Sort the unused tokens by ID to find the lowest
            let mut sorted_unused_tokens = unused_tokens.clone();
            sorted_unused_tokens.sort_by_key(|token| token.id);

            unused_objects_per_type.insert(object_type.clone(), sorted_unused_tokens);
        }

        // Now, filter the firing combinations
        firing_combinations
            .iter()
            .filter(|combination| {
                // For each object type in the combination, apply the following:
                combination
                    .object_binding_info
                    .iter()
                    .all(|(object_type, binding_info)| {
                        // Assuming each binding_info has exactly one token
                        let token = binding_info.tokens.first().unwrap();
                        let object_id = self.token_graph_id_mapping.get(&token.id).unwrap();

                        // Check if the object is already used
                        let is_used = !node
                            .partial_case
                            .adjacency
                            .get(object_id)
                            .map_or(true, |edges| edges.is_empty());

                        if is_used {
                            // If the object is already used, any is allowed
                            return true;
                        } else {
                            // Find the lowest unused token for this object type
                            let unused_tokens = unused_objects_per_type.get(object_type).unwrap();
                            // The lowest unused token is the first in the sorted list
                            let lowest_unused = unused_tokens.first().unwrap();
                            // Ensure that this combination includes the lowest unused token^    ^
                            return token.id == lowest_unused.id;
                        }
                    })
            })
            .cloned()
            .collect()
    }
    #[inline(never)]
    fn generate_children<'a>(
        &mut self,
        node: &SearchNode,
        query_case_stats: &CaseStats,
        query_case: &'a CaseGraph,
        static_cost: f64,
    ) -> Vec<SearchNode> {
        //let mut children = Vec::new();

        // only add tokens to initial places, if the node is the initial node or follows a add token action node
        //println!("GEtting initial places");
        // get last entry of node.action_path
        /* FIXME
                if (node.action_path.last().unwrap().is_pre_firing()) {
                    //println!("Pre firing");
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
                            .get(&a.oc_object_type)
                            .unwrap_or(&0);
                        let b_query_count = query_case_stats
                            .query_object_counts
                            .get(&b.oc_object_type)
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
                                object_type: place.object_type.clone().into(),
                            });
                            new_partial_case.add_node(new_object);

                            let mut new_partial_case_stats = node.partial_case_stats.clone();
                            new_partial_case_stats
                                .query_object_counts
                                .entry(place.oc_object_type)
                                .and_modify(|e| *e += 1)
                                .or_insert(1);

                            let min_cost = self.calculate_min_cost(
                                &query_case_stats,
                                &new_partial_case_stats,
                                static_cost,
                                &new_marking,
                            );
                            self.token_graph_id_mapping.insert(token_ids[0], object_id);
                            let mut new_action_path = node.action_path.clone();
                            let new_action = Arc::new(SearchNodeAction::add_token(place.id.clone()));
                            new_action_path.push(new_action);
                            children.push(SearchNode::new_with_stats(
                                new_marking,
                                new_partial_case,
                                min_cost,
                                None,
                                new_action_path,
                                new_partial_case_stats,
                                node.depth + 1,
                                None,
                            ));
                        }
                        // If higher_types_used is true, do not add tokens to this and lower types
                        // Continue to the next place
                    }
                }
        */
        //println!("Getting transitions");

        //
        let mut transition_enabled: HashMap<Uuid, bool> = HashMap::new();

        let mut firing_combinations_per_transition: HashMap<Uuid, Vec<Arc<Binding>>> =
            HashMap::new();

        let mut forbidden_firings_per_transition: HashMap<Uuid, Vec<Arc<Binding>>> = HashMap::new();

        let mut transition_children: Vec<SearchNode> = Vec::new();
        for transition in self.model.transitions.values() {
            //println!("Transition: {}", transition.name);
            let firing_combinations = node.marking.get_firing_combinations(transition);
            if (firing_combinations.len() > 0) {
                //println!("{}",transition.name);
                //println!("Firing combinations: {:?}", firing_combinations.iter().map(|c| c.to_string()).collect::<Vec<String>>());
            }

            transition_enabled.insert(transition.id, firing_combinations.len() > 0);
            let firing_combinations: Vec<Arc<Binding>> =
                firing_combinations.into_iter().map(Arc::new).collect();

            if (!transition.silent) {
                forbidden_firings_per_transition
                    .insert(transition.id, firing_combinations.iter().cloned().collect());
            }

            firing_combinations_per_transition.insert(transition.id, firing_combinations);
        }

        if let Some(forbidden_firings_previous) = &node.forbidden_firings {
            // if this is the case, merge forbidden_firings_per_transition with forbidden_firings_previous
            for (transition_id, forbidden_firings_to_extend) in forbidden_firings_previous {
                let forbidden_firings = forbidden_firings_per_transition
                    .entry(*transition_id)
                    .or_insert_with(|| vec![]);
                forbidden_firings.extend(forbidden_firings_to_extend.iter().cloned());
            }
        }

        for transition in self.model.transitions.values() {
            let firing_combinations = firing_combinations_per_transition
                .get(&transition.id)
                .unwrap();
            if firing_combinations.is_empty() {
                continue;
            }

            // Filter combinations based on symmetry breaking
            let filtered_combinations = firing_combinations;
            //self.filter_firing_combinations(&firing_combinations, transition, node);

            if (filtered_combinations.is_empty()) {
                panic!("No filtered combinations?!");
            }

            if (filtered_combinations.len() < firing_combinations.len()) {
                //println!("Filtered out {} combinations", firing_combinations.len() - filtered_combinations.len());
            }

            // filter out forbidden firings, meaning filter out all bindings from filteredCombinations that are in searchNode.forbiddenFirings

            let mut filtered_forbidden_combinations = match &node.forbidden_firings {
                Some(forbidden_firings_per_t) => {
                    let forbidden_firings = forbidden_firings_per_t.get(&transition.id);
                    if (forbidden_firings.is_none()) {
                        filtered_combinations.clone()
                    } else {
                        let forbidden_firings = forbidden_firings.unwrap();
                        filtered_combinations
                            .iter()
                            .filter_map(|combination| {
                                if (forbidden_firings.contains(combination)) {
                                    return None;
                                }
                                return Some(combination);
                            })
                            .cloned()
                            .collect()
                    }
                }
                None => filtered_combinations.clone(),
            };
            let filtered_out_len =
                filtered_combinations.len() - filtered_forbidden_combinations.len();
            if (filtered_out_len > 0) {
                //println!("Filtered out {} forbidden combinations", filtered_out_len);
            }

            if (transition.silent) {
                filtered_forbidden_combinations.sort_by(|a, b| compare_bindings(a, b));
            }

            filtered_forbidden_combinations
                .iter()
                .enumerate()
                .for_each(|(index, combination)| {
                    let mut new_marking = node.marking.clone();
                    let mut new_partial_case = node.partial_case.clone();

                    let mut most_recent_event_id = node.most_recent_event_id.clone();

                    new_marking.fire_transition(transition, combination);
                    //let mut new_partial_case_stats = node.partial_case.get_case_stats();

                    let mut new_action_path = node.action_path.clone();
                    let new_action = Arc::new(SearchNodeAction::fire_transition(
                        transition.id,
                        combination.clone().clone(),
                    ));
                    new_action_path.push(new_action);

                    if (!transition.silent) {
                        let event_type: EventType = transition.event_type.clone().into();
                        let event_id = new_partial_case.get_new_id();
                        let new_event = Node::EventNode(Event {
                            id: event_id,
                            event_type: event_type.clone(),
                        });
                        new_partial_case.add_node(new_event);
                        let mut collected_edges: Vec<Edge> = Vec::new();
                        combination
                            .object_binding_info
                            .values()
                            .for_each(|binding_info| {
                                binding_info.tokens.iter().for_each(|token| {
                                    let e20_edge_id = new_partial_case.get_new_id();
                                    collected_edges.push(Edge::new(
                                        e20_edge_id,
                                        event_id,
                                        self.token_graph_id_mapping.get(&token.id).unwrap().clone(),
                                        EdgeType::E2O,
                                    ));
                                    // new_partial_case_stats
                                    //     .query_edge_counts
                                    //     .entry(EdgeType::E2O)
                                    //     .and_modify(|e| *e += 1)
                                    //     .or_insert(1);
                                    //
                                    // *new_partial_case_stats
                                    //     .edge_type_counts
                                    //     .entry((
                                    //         EdgeType::E2O,
                                    //         transition.event_type.into(),
                                    //         binding_info.object_type.into(),
                                    //     ))
                                    //     .or_insert(0) += 1;
                                })
                            });

                        let df_edge_id = new_partial_case.get_new_id();
                        if let Some(prev_event_id) = node.most_recent_event_id {
                            collected_edges.push(Edge::new(
                                df_edge_id,
                                prev_event_id,
                                event_id,
                                EdgeType::DF,
                            ));
                            // new_partial_case_stats
                            //     .query_edge_counts
                            //     .entry(EdgeType::DF)
                            //     .and_modify(|e| *e += 1)
                            //     .or_insert(1);
                            //
                            // let a = new_partial_case
                            //     .get_node(prev_event_id)
                            //     .unwrap()
                            //     .oc_type_id();
                            // let b = transition.event_type.into();
                            //
                            // *new_partial_case_stats
                            //     .edge_type_counts
                            //     .entry((EdgeType::DF, a, b))
                            //     .or_insert(0) += 1;
                        }

                        // new_partial_case_stats
                        //     .query_event_counts
                        //     .entry(transition.event_type)
                        //     .and_modify(|e| *e += 1)
                        //     .or_insert(1);

                        most_recent_event_id = Some(event_id);

                        collected_edges.iter().for_each(|edge| {
                            new_partial_case.add_edge(edge.clone());
                        });

                        // now there's the following options for a new search node: first, all open nodes matching the event cann be used as mapping
                        // second, the new node here is not mapped to the query nodes
                        node.open_query_nodes
                            .iter()
                            .filter(|node| {
                                query_case.nodes.get(node).unwrap().oc_type_id() == event_type.0
                            })
                            .for_each(|open_node| {
                                let mut new_open_node_list = node.open_query_nodes.clone();
                                let mut new_reverse_node_mapping =
                                    node.reverse_node_mapping.clone();
                                let mut new_reverse_edge_mapping =
                                    node.reverse_edge_mapping.clone();
                                // remove open node, it is an element not an index
                                new_open_node_list.retain(|n| n != open_node);

                                // add new node to mapping
                                new_reverse_node_mapping
                                    .insert(event_id, RealNode(event_id, *open_node));

                                let mut edges_hit: Vec<usize> = Vec::new();

                                let mut void_edge_count = 0;
                                // check which edges can be assigned and which edges cannot
                                collected_edges.iter().for_each(|edge| {
                                    // if both nodes are corresponding in the mapping, we add it as a realEdge, otherwise as a void Edge
                                    // for that, we look at the node mapping for each node connected to the edge
                                    let a = new_reverse_node_mapping.get(&edge.from).unwrap();
                                    let b = new_reverse_node_mapping.get(&edge.to).unwrap();

                                    if a.is_void() || b.is_void() {
                                        new_reverse_edge_mapping
                                            .insert(edge.id, VoidEdge(edge.id, edge.id));
                                        void_edge_count += 1;
                                    } else {
                                        // now, we need to first identify the corresponding edge in the query case graph to make a mapping possible
                                        // for that, we need to get the node ids of the edge

                                        // there should at maximum be one edge between the two nodes
                                        if let Some(query_edge) = query_case
                                            .get_edge_between(a.target_id(), b.target_id())
                                        {
                                            new_reverse_edge_mapping
                                                .insert(edge.id, RealEdge(edge.id, query_edge.id));
                                            edges_hit.push(query_edge.id);
                                        } else {
                                            // Handle the case where the edge was not found
                                            void_edge_count += 1;
                                            new_reverse_edge_mapping
                                                .insert(edge.id, VoidEdge(edge.id, edge.id));
                                        }
                                    }
                                });

                                let void_cost = query_case
                                    .get_connected_edges(*open_node)
                                    .unwrap()
                                    .iter()
                                    .map(|edge| query_case.get_edge(*edge).unwrap())
                                    .filter(|edge| {
                                        // edge is a df edge to the next node - we handle that in the next node coming
                                        // fixme really? what if this is the very last node - then that should also cause cost, right?
                                        if (edge.edge_type == EdgeType::DF  && edge.from == *open_node)
                                        {
                                            return false;
                                        }
                                        return !edges_hit.contains(&edge.id);
                                    })
                                    .count();

                                let new_min_cost = node.static_min_cost
                                    + void_edge_count as f64 + void_cost as f64;

                                let min_remaining_cost = self.calculate_unreachable_events(
                                    &new_marking,
                                    &new_open_node_list,
                                    query_case,
                                );

                                if (node.min_cost > new_min_cost + min_remaining_cost as f64) {
                                    println!("Old lb {}", node.min_cost);
                                    println!("New lb {}", new_min_cost + min_remaining_cost as f64);
                                    println!("share of heuristic {}", min_remaining_cost);
                                    println!(
                                        "Old share of heuristic {}",
                                        node.min_cost - node.static_min_cost
                                    );
                                    let cost_old = self.calculate_unreachable_events_verbose(
                                        &node.marking,
                                        &node.open_query_nodes,
                                        query_case,
                                    );
                                    let cost = self.calculate_unreachable_events_verbose(
                                        &new_marking,
                                        &new_open_node_list,
                                        query_case,
                                    );
                                    println!("Cost {}", cost);
                                    panic!("Lower bound decreased wtf1!")
                                }

                                transition_children.push(SearchNode::new_with_stats(
                                    new_marking.clone(),
                                    new_partial_case.clone(),
                                    new_min_cost + min_remaining_cost as f64,
                                    new_min_cost,
                                    most_recent_event_id,
                                    new_action_path.clone(),
                                    None,
                                    node.depth + 1,
                                    new_open_node_list,
                                    new_reverse_node_mapping,
                                    new_reverse_edge_mapping,
                                    None,
                                    // node.epsilon_cost + void_edge_count as f64,
                                    // node.void_cost + void_cost as f64,
                                ));
                            });

                        // now second
                        {
                            let mut new_reverse_node_mapping = node.reverse_node_mapping.clone();
                            let mut new_reverse_edge_mapping = node.reverse_edge_mapping.clone();
                            // remove open node, it is an element not an index

                            // add new node to mapping
                            new_reverse_node_mapping.insert(event_id, VoidNode(event_id, event_id));
                            collected_edges.iter().for_each(|edge| {
                                new_reverse_edge_mapping
                                    .insert(edge.id, VoidEdge(edge.id, edge.id));
                            });

                            let mut new_min_cost =
                                node.static_min_cost + collected_edges.len() as f64 + 1.0;
                            let min_remaining_cost = self.calculate_unreachable_events(
                                &new_marking,
                                &node.open_query_nodes,
                                query_case,
                            );

                            if (node.min_cost > new_min_cost + min_remaining_cost as f64) {
                                println!("Old lb {}", node.min_cost);
                                println!("New lb {}", new_min_cost + min_remaining_cost as f64);
                                println!("share of heuristic {}", min_remaining_cost);
                                println!(
                                    "Old share of heuristic {}",
                                    node.min_cost - node.static_min_cost
                                );
                                panic!("Lower bound decreased wtf2!")
                            }

                            transition_children.push(SearchNode::new_with_stats(
                                new_marking,
                                new_partial_case,
                                new_min_cost + min_remaining_cost as f64,
                                new_min_cost,
                                most_recent_event_id,
                                new_action_path,
                                None,
                                node.depth + 1,
                                node.open_query_nodes.clone(),
                                new_reverse_node_mapping,
                                new_reverse_edge_mapping,
                                None,
                                // node.epsilon_cost + collected_edges.len() as f64 + 1.0,
                                // node.void_cost +void_cost,
                            ));
                        }
                    } else {
                        let forbidden_firings = {
                            let mut base = forbidden_firings_per_transition.clone();
                            // add all combinations from index 0 to index to forbidden firings
                            // select them from filtered_forbidden_combinations
                            // -> symmetry-breaking
                            let forbidden_firings =
                                filtered_forbidden_combinations[..index].to_vec();
                            if (!forbidden_firings.is_empty()) {
                                base.entry(transition.id)
                                    .or_insert_with(|| vec![])
                                    .extend(forbidden_firings);
                            }
                            // now, also forbid all combinations from all silent transitions with a higher lexicographical order
                            // which are also firable in this node
                            // first, sort all silent transitions lexicograhically by name

                            let mut sorted_silent_transitions: Vec<&Transition> = self
                                .model
                                .transitions
                                .values()
                                .filter(|t| t.silent)
                                .collect();
                            sorted_silent_transitions.sort_by(|a, b| a.id.cmp(&b.id));

                            // now, iterate over all silent transitions with a higher lexicographical order
                            // and add all firable combinations to the forbidden firings
                            for silent_transition in sorted_silent_transitions {
                                if (silent_transition.id <= transition.id) {
                                    continue;
                                }
                                let firable_combinations = firing_combinations_per_transition
                                    .get(&silent_transition.id)
                                    .unwrap();
                                // todo consider filtering out forbidden firings
                                base.entry(silent_transition.id)
                                    .or_insert_with(|| vec![])
                                    .extend(firable_combinations.iter().cloned());
                            }

                            Some(base)
                        };

                        let min_remaining_cost = self.calculate_unreachable_events(
                            &new_marking,
                            &node.open_query_nodes,
                            query_case,
                        );

                        if (node.min_cost > node.static_min_cost + min_remaining_cost as f64) {
                            println!("Old lb {}", node.min_cost);
                            println!(
                                "New lb {}",
                                node.static_min_cost + min_remaining_cost as f64
                            );
                            println!("share of heuristic {}", min_remaining_cost);
                            println!(
                                "Old share of heuristic {}",
                                node.min_cost - node.static_min_cost
                            );
                            panic!("Lower bound decreased wtf3!")
                        }

                        transition_children.push(SearchNode::new_with_stats(
                            new_marking,
                            new_partial_case,
                            node.static_min_cost + min_remaining_cost as f64,
                            node.static_min_cost,
                            most_recent_event_id,
                            new_action_path,
                            None,
                            node.depth + 1,
                            node.open_query_nodes.clone(),
                            node.reverse_node_mapping.clone(),
                            node.reverse_edge_mapping.clone(),
                            forbidden_firings,
                            // node.epsilon_cost,
                            // node.void_cost
                        ));
                    }

                    /*let new_cost = self.calculate_min_cost(
                        &query_case_stats,
                        &new_partial_case_stats,
                        static_cost,
                        &new_marking,
                    );*/
                    // fixme
                    /*
                    let new_cost = 99999999.0;
                    if (new_cost < node.min_cost) {
                        println!("!!!!! COST DECREASED by {} WTF", node.min_cost - new_cost);
                        self.more_epsilon_than_void_with_debug(&node.marking);
                        for ((edge_type, a, b), &partial_count) in
                            &node.partial_case_stats.edge_type_counts
                        {
                            if edge_type.ne(&EdgeType::E2O) {
                                continue;
                            }
                            let query_count = query_case_stats
                                .edge_type_counts
                                .get(&(*edge_type, *a, *b))
                                .unwrap_or(&0);
                            let type_storage = TYPE_STORAGE.read().unwrap();
                            println!(
                                "Edge: ({:?},{},{}) Difference: {}",
                                edge_type,
                                type_storage.get_type_name(*a).unwrap(),
                                type_storage.get_type_name(*b).unwrap(),
                                (partial_count as f64 - *query_count as f64)
                            );
                        }
                        println!("-----------------");
                        println!("After firing transition: {}", transition.name);
                        for ((edge_type, a, b), &partial_count) in
                            &new_partial_case_stats.edge_type_counts
                        {
                            if edge_type.ne(&EdgeType::E2O) {
                                continue;
                            }
                            let query_count = query_case_stats
                                .edge_type_counts
                                .get(&(*edge_type, *a, *b))
                                .unwrap_or(&0);
                            let type_storage = TYPE_STORAGE.read().unwrap();
                            println!(
                                "Edge: ({:?},{},{}) Difference: {}",
                                edge_type,
                                type_storage.get_type_name(*a).unwrap(),
                                type_storage.get_type_name(*b).unwrap(),
                                (partial_count as f64 - *query_count as f64)
                            );
                        }
                        self.more_epsilon_than_void_with_debug(&new_marking);
                        println!("-----------------");
                        query_case_stats.pretty_print_stats()
                    }*/
                });
        }
        //println!("done getting transitions");
        // places are allowed to be dead as long as we keep adding tokens to alive them ;)
        if (!node.action_path.last().unwrap().is_pre_firing()) {
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
        /*transition_children.sort_by(|a, b| {
            // prioritize transitions that have a higher difference between the case stats and the query case stats
            // all actions in this list are transitions
            let transition_a = a.action_path.last().unwrap().transition_id().unwrap();

            let transition_type = &self.model.get_transition(&transition_a).unwrap().event_type;

            let difference_a = *query_case_stats
                .query_event_counts
                .get(transition_type)
                .unwrap_or(&0) as i64
                - *a.partial_case_stats
                    .query_event_counts
                    .get(transition_type)
                    .unwrap_or(&0) as i64;

            let transition_b = b.action_path.last().unwrap().transition_id().unwrap();
            let transition_type = &self.model.get_transition(&transition_b).unwrap().event_type;

            let difference_b = *query_case_stats
                .query_event_counts
                .get(transition_type)
                .unwrap_or(&0) as i64
                - *b.partial_case_stats
                    .query_event_counts
                    .get(transition_type)
                    .unwrap_or(&0) as i64;

            difference_a.partial_cmp(&difference_b).unwrap()
        });*/
        return transition_children;
        //children.append(&mut transition_children);
        //children
    }
}
fn compare_bindings(a: &Arc<Binding>, b: &Arc<Binding>) -> Ordering {
    // Collect and sort object types once
    let mut object_types: Vec<&ObjectType> = a.object_binding_info.keys().collect();
    object_types.sort_unstable();

    for object_type in object_types {
        let a_tokens = &a.object_binding_info[object_type].tokens;
        let b_tokens = &b.object_binding_info[object_type].tokens;

        // Check if the number of tokens differs
        if a_tokens.len() != b_tokens.len() {
            return a_tokens.len().cmp(&b_tokens.len());
        }

        // Compare sorted token IDs
        let mut a_sorted = a_tokens.iter().map(|t| t.id).collect::<Vec<_>>();
        let mut b_sorted = b_tokens.iter().map(|t| t.id).collect::<Vec<_>>();
        a_sorted.sort_unstable();
        b_sorted.sort_unstable();

        for (&a_id, &b_id) in a_sorted.iter().zip(b_sorted.iter()) {
            match a_id.cmp(&b_id) {
                Ordering::Less => return Ordering::Less,
                Ordering::Greater => return Ordering::Greater,
                Ordering::Equal => continue,
            }
        }
    }

    // All object types and their tokens are equal
    Ordering::Equal
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
    use std::path::Path;
    use std::{fs, panic};

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
        // Wrap the petri net in Arc to match branch_and_bound signature
        let petri_net_arc = Arc::new(petri_net);

        let initial_marking = Marking::new(petri_net_arc.clone());

        // Create a CaseGraph representing the query case
        let mut query_case = CaseGraph::new();

        // Add nodes corresponding to the events in the query case
        let event1 = Node::EventNode(Event {
            id: 1,
            event_type: "T1".into(),
        });
        let event2 = Node::EventNode(Event {
            id: 2,
            event_type: "T2".into(),
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
        let result = panic::catch_unwind(|| {
            let json_data =
                fs::read_to_string("./src/oc_align/test_data/bpi17/oc_petri_net.json").unwrap();
            let ocpn = initialize_ocpn_from_json(&json_data);

            // Wrap the petri net in Arc to match branch_and_bound signature
            let petri_net_arc = Arc::new(ocpn);
            let initial_marking = Marking::new(petri_net_arc.clone());

            // deserialize a query case from a json file
            // load file as string

            let shortest_case_json =
                fs::read_to_string("./src/oc_align/test_data/bpi17/shortest_case_graph.json")
                    .expect("Unable to read file");
            let shortest_case = deserialize_case_graph(shortest_case_json.as_str());

            // Initialize ModelCaseChecker
            let mut checker =
                ModelCaseChecker::new_with_shortest_case(petri_net_arc.clone(), shortest_case);

            let case_graph_iter = CaseGraphIterator::new(
                "/Users/erikwrede/dev/uni/ma-py/ocgc-py/ocgc/problemkinder/bound",
            )
            .unwrap();
            let visualized_dir =
                Path::new("/Users/erikwrede/dev/uni/ma-py/ocgc-py/ocgc/varsbpi_visualized");
            for (case_graph, path) in case_graph_iter {
                let output_file_name = path.file_stem().unwrap().to_str().unwrap().to_owned();
                let output_path = visualized_dir.join(output_file_name);

                /*                export_case_graph_image(
                    &case_graph,
                    output_path.to_str().unwrap().to_owned() + "_query.png",
                    Format::Png,
                    Some(2.0),
                )
                    .unwrap();*/

                let result = checker.bnb_v2(&case_graph, initial_marking.clone());
                if let Some(result_node) = result {
                    println!("Solution found for case {:?}", path);
                    // save the alignment result as an image in a directory next to /Users/erikwrede/dev/uni/ma-py/ocgc-py/ocgc/varsbpi
                    let alignment =
                        CaseAlignment::align_mip(&case_graph, &result_node.partial_case);
                    let cost = alignment.total_cost().unwrap_or(f64::INFINITY);
                    println!("Cost: {}", cost);
                    export_c2_with_alignment_image(
                        &result_node.partial_case,
                        &alignment,
                        output_path.to_str().unwrap().to_owned()
                            + format!("_aligned_cost_{}.png", cost).as_str(),
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
        });
        if result.is_err() {
            println!("Error: {:?}", result.err());
        }
    }

    #[test]
    fn save_case_stats() {
        let result = panic::catch_unwind(|| {
            let json_data =
                fs::read_to_string("./src/oc_align/test_data/bpi17/oc_petri_net.json").unwrap();
            let ocpn = initialize_ocpn_from_json(&json_data);

            // Wrap the petri net in Arc to match branch_and_bound signature
            let petri_net_arc = Arc::new(ocpn);
            let initial_marking = Marking::new(petri_net_arc.clone());

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
                CaseGraphIterator::new("/Users/erikwrede/dev/uni/ma-py/ocgc-py/test_data/varsbpi")
                    .unwrap();
            // ensure directory stats exists or create it
            let stats_dir = Path::new("/Users/erikwrede/dev/uni/ma-py/ocgc-py/ocgc/stats");
            if !stats_dir.exists() {
                fs::create_dir(stats_dir).unwrap();
            }
            for (case_graph, path) in case_graph_iter {
                let output_file_name = path.file_stem().unwrap().to_str().unwrap().to_owned();
                let output_path = stats_dir.join(output_file_name);

                let case_stats = case_graph.get_case_stats();

                let object_count = case_graph.nodes.values().filter(|n| n.is_object()).count();
                let event_count = case_graph.nodes.values().filter(|n| n.is_event()).count();

                let case_size = case_graph.nodes.len() + case_graph.edges.len();

                // save object_count, event_count, case_size, case_stats to a json file at output path

                let mut stats = HashMap::new();
                stats.insert("object_count", object_count);
                stats.insert("event_count", event_count);
                stats.insert("case_size", case_size);

                let case_stats_json = serde_json::to_string(&stats).unwrap();

                fs::write(
                    output_path.to_str().unwrap().to_owned() + "_stats.json",
                    case_stats_json,
                )
                .unwrap();
            }
        });
        if result.is_err() {
            println!("Error: {:?}", result.err());
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

        // Wrap the petri net in Arc
        let petri_net_arc = Arc::new(petri_net);
        let initial_marking = Marking::new(petri_net_arc.clone());

        // Create a CaseGraph representing the query case (missing optional path)
        let mut query_case = CaseGraph::new();

        // Add nodes corresponding to the events in the query case
        let event1 = Node::EventNode(Event {
            id: 1,
            event_type: "T1".into(),
        });
        let event2 = Node::EventNode(Event {
            id: 2,
            event_type: "T2".into(),
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
