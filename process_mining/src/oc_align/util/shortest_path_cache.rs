// shortest_path_cache.rs

use std::collections::{HashMap, BinaryHeap};
use std::sync::{Arc, Mutex};
use parking_lot::RwLock;
use uuid::Uuid;
use crate::oc_petri_net::oc_petri_net::ObjectCentricPetriNet;

/// Struct to represent the result of a shortest path query.
/// Contains the path as a vector of place IDs and the distance
/// (# of non-silent transitions) along the path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortestPathResult {
    pub path: Vec<Uuid>,
    pub distance: usize,
}

/// Struct to cache shortest path information between places in the Petri net.
#[derive(Debug)]
pub struct ShortestPathCache {
    /// Reference to the Petri net.
    petri_net: Arc<ObjectCentricPetriNet>,
    /// Cache storing shortest path results. The key is a tuple of (from_place_id, to_place_id).
    /// The value is an Option containing ShortestPathResult. None signifies no path exists.
    cache: RwLock<HashMap<(Uuid, Uuid), Option<ShortestPathResult>>>,
}

impl ShortestPathCache {
    /// Creates a new ShortestPathCache associated with a given Petri net.
    pub fn new(petri_net: Arc<ObjectCentricPetriNet>) -> Self {
        ShortestPathCache {
            petri_net,
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// Retrieves the shortest path from `from_place_id` to `to_place_id`.
    /// Utilizes caching to store and retrieve previous computations.
    /// Returns `None` if no path exists.
    /// `distance` indicates the number of non-silent transitions on the path.
    pub fn shortest_path(&self, from_place_id: &Uuid, to_place_id: &Uuid) -> Option<ShortestPathResult> {
        // If both places are the same, return the trivial path with distance 0.
        if from_place_id == to_place_id {
            return Some(ShortestPathResult {
                path: vec![*from_place_id],
                distance: 0,
            });
        }

        // Acquire the lock to access the cache.
        {
            let cache_guard = self.cache.read();
            if let Some(cached_result) = cache_guard.get(&(*from_place_id, *to_place_id)) {
                return cached_result.clone();
            }
        }

        // Perform the search to find the shortest path.
        let result = self.find_shortest_path(from_place_id, to_place_id);

        // Store the result in the cache.
        let mut cache_guard = self.cache.write();
        cache_guard.insert((*from_place_id, *to_place_id), result.clone());

        result
    }

    /// Finds the shortest path from `from_place_id` to `to_place_id` using a modified BFS.
    /// Silent transitions are traversed with distance 0.
    /// Only traverses through places with the same `object_type`.
    fn find_shortest_path(&self, from_place_id: &Uuid, to_place_id: &Uuid) -> Option<ShortestPathResult> {
        // Retrieve the starting and target places.
        let start_place = self.petri_net.get_place(from_place_id)?;
        let target_object_type = start_place.object_type.clone();

        // Priority queue to explore paths with the smallest distance first.
        // BinaryHeap in Rust is a max-heap, so we invert the distance for min-heap behavior.
        let mut heap = BinaryHeap::new();
        heap.push(State {
            place_id: *from_place_id,
            distance: 0,
            path: vec![*from_place_id],
        });

        // Visited map to keep track of the smallest distance to each place.
        let mut visited: HashMap<Uuid, usize> = HashMap::new();
        visited.insert(*from_place_id, 0);

        while let Some(State { place_id, distance, path }) = heap.pop() {
            // Check if we've reached the target.
            if &place_id == to_place_id {
                return Some(ShortestPathResult {
                    path,
                    distance,
                });
            }

            // Get the current place.
            let current_place = match self.petri_net.get_place(&place_id) {
                Some(p) => p,
                None => continue, // If place doesn't exist, skip.
            };
            let current_object_type = &current_place.object_type;

            // Ensure we're only traversing through places with the same object_type.
            if current_object_type != &target_object_type {
                continue;
            }

            // Iterate over all outgoing input arcs (i.e., transitions connected to this place).
            for input_arc in &current_place.output_arcs {
                let transition_id = input_arc.target_transition_id;

                // Get the transition.
                let transition = match self.petri_net.get_transition(&transition_id) {
                    Some(t) => t,
                    None => continue, // If transition doesn't exist, skip.
                };

                // Iterate over all output arcs of the transition to reach new places.
                for output_arc in &transition.output_arcs {
                    let next_place_id = output_arc.target_place_id;
                    let next_place = match self.petri_net.get_place(&next_place_id) {
                        Some(p) => p,
                        None => continue, // If place doesn't exist, skip.
                    };

                    // Ensure the next place has the same object_type.
                    if start_place.object_type != next_place.object_type {
                        continue;
                    }

                    // Calculate the new distance.
                    // Non-silent transitions increment the distance by 1.
                    let new_distance = if transition.silent {
                        distance
                    } else {
                        distance + 1
                    };

                    // If this path to next_place_id is shorter, consider it.
                    if let Some(&existing_distance) = visited.get(&next_place_id) {
                        if new_distance >= existing_distance {
                            continue;
                        }
                    }

                    // Update visited with the new shorter distance.
                    visited.insert(next_place_id, new_distance);

                    // Create the new path.
                    let mut new_path = path.clone();
                    new_path.push(next_place_id);

                    // Push the new state onto the heap.
                    heap.push(State {
                        place_id: next_place_id,
                        distance: new_distance,
                        path: new_path,
                    });
                }
            }
        }

        // If the target is not reachable, return None.
        None
    }
}

/// Struct to represent the state in the priority queue.
#[derive(Debug)]
struct State {
    place_id: Uuid,
    distance: usize,
    path: Vec<Uuid>,
}

// Implement ordering for the priority queue (min-heap based on distance).
impl PartialEq for State {
    fn eq(&self, other: &Self) -> bool {
        self.distance == other.distance
    }
}

impl Eq for State {}

impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        // Reverse the order for min-heap.
        Some(other.distance.cmp(&self.distance))
    }
}

impl Ord for State {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse the order for min-heap.
        other.distance.cmp(&self.distance)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use crate::oc_petri_net::initialize_ocpn_from_json;
    use crate::oc_petri_net::oc_petri_net::Place;
    use super::*;

    #[test]
    fn test_shortest_path_simple() {
        let mut net = ObjectCentricPetriNet::new();

        // Add places with the same object_type.
        let p1 = net.add_place(Some("P".to_string()), "A".to_string(), true, false);
        let p2 = net.add_place(Some("Q".to_string()), "A".to_string(), false, false);
        let p3 = net.add_place(Some("R".to_string()), "A".to_string(), false, false);

        // Add transitions
        let t1 = net.add_transition("T1".to_string(), Some("Transition 1".to_string()), false);
        let t2 = net.add_transition("T2".to_string(), None, true);
        let t3 = net.add_transition("T3".to_string(), Some("Transition 3".to_string()), false);

        // Add arcs: p1 -> t1 -> p2 -> t2 -> p3
        net.add_input_arc(p1.id, t1.id, false, 1);
        net.add_output_arc(t1.id, p2.id, false, 1);
        net.add_input_arc(p2.id, t2.id, true, 1);
        net.add_output_arc(t2.id, p3.id, true, 1);

        // Additional arcs to form alternative paths.
        // p1 -> t3 -> p3 (non-silent transition)
        net.add_input_arc(p1.id, t3.id, false, 1);
        net.add_output_arc(t3.id, p3.id, false, 1);

        let petri_net = Arc::new(net);
        let shortest_path_cache = ShortestPathCache::new(Arc::clone(&petri_net));

        // Shortest path from p1 to p3 should be [p1, p3] with distance 1
        let path_result = shortest_path_cache.shortest_path(&p1.id, &p3.id);
        assert_eq!(
            path_result,
            Some(ShortestPathResult {
                path: vec![p1.id, p3.id],
                distance: 1
            })
        );

        // Shortest path from p1 to p2 should be [p1, p2] with distance 1
        let path_result = shortest_path_cache.shortest_path(&p1.id, &p2.id);
        assert_eq!(
            path_result,
            Some(ShortestPathResult {
                path: vec![p1.id, p2.id],
                distance: 1
            })
        );

        // Shortest path from p2 to p3 should be [p2.id, p3.id] with distance 0 (only through silent transition)
        let path_result = shortest_path_cache.shortest_path(&p2.id, &p3.id);
        assert_eq!(
            path_result,
            Some(ShortestPathResult {
                path: vec![p2.id, p3.id],
                distance: 0
            })
        );

        // No path from p3 to p1
        let path_result = shortest_path_cache.shortest_path(&p3.id, &p1.id);
        assert_eq!(path_result, None);
    }

    #[test]
    fn test_shortest_path_with_labels() {
        let mut net = ObjectCentricPetriNet::new();

        // Add places with different object_types.
        let p1 = net.add_place(Some("P1".to_string()), "Label1".to_string(), true, false);
        let p2 = net.add_place(Some("P2".to_string()), "Label1".to_string(), false, false);
        let p3 = net.add_place(Some("P3".to_string()), "Label2".to_string(), false, false);
        let p4 = net.add_place(Some("P4".to_string()), "Label1".to_string(), false, false);

        // Add transitions
        let t1 = net.add_transition("T1".to_string(), Some("Transition 1".to_string()), false);
        let t2 = net.add_transition("T2".to_string(), None, true);
        let t3 = net.add_transition("T3".to_string(), Some("Transition 3".to_string()), false);

        // Add arcs:
        // p1 -> t1 -> p2 -> t2 -> p3 (different object_type)
        // p2 -> t3 -> p4
        net.add_input_arc(p1.id, t1.id, false, 1);
        net.add_output_arc(t1.id, p2.id, false, 1);
        net.add_input_arc(p2.id, t2.id, true, 1);
        net.add_output_arc(t2.id, p3.id, true, 1);
        net.add_input_arc(p2.id, t3.id, false, 1);
        net.add_output_arc(t3.id, p4.id, false, 1);

        let petri_net = Arc::new(net);
        let shortest_path_cache = ShortestPathCache::new(Arc::clone(&petri_net));

        // Path from p1 to p4 should exist: [p1, p2, p4] with distance 2 (t1: non-silent, t3: non-silent)
        let path_result = shortest_path_cache.shortest_path(&p1.id, &p4.id);
        assert_eq!(
            path_result,
            Some(ShortestPathResult {
                path: vec![p1.id, p2.id, p4.id],
                distance: 2
            })
        );

        // Path from p1 to p3 should not exist due to different object_types
        let path_result = shortest_path_cache.shortest_path(&p1.id, &p3.id);
        assert_eq!(path_result, None);
    }

    #[test]
    fn test_shortest_path_cycles() {
        let mut net = ObjectCentricPetriNet::new();

        // Add places with the same object_type.
        let p1 = net.add_place(Some("P1".to_string()), "Cycle".to_string(), true, false);
        let p2 = net.add_place(Some("P2".to_string()), "Cycle".to_string(), false, false);
        let p3 = net.add_place(Some("P3".to_string()), "Cycle".to_string(), false, false);

        // Add transitions forming a cycle: p1 -> t1 -> p2 -> t2 -> p3 -> t3 -> p1
        let t1 = net.add_transition("T1".to_string(), Some("Transition 1".to_string()), false);
        let t2 = net.add_transition("T2".to_string(), Some("Transition 2".to_string()), true);
        let t3 = net.add_transition("T3".to_string(), Some("Transition 3".to_string()), false);

        net.add_input_arc(p1.id, t1.id, false, 1);
        net.add_output_arc(t1.id, p2.id, false, 1);
        net.add_input_arc(p2.id, t2.id, true, 1);
        net.add_output_arc(t2.id, p3.id, true, 1);
        net.add_input_arc(p3.id, t3.id, false, 1);
        net.add_output_arc(t3.id, p1.id, false, 1);

        let petri_net = Arc::new(net);
        let shortest_path_cache = ShortestPathCache::new(Arc::clone(&petri_net));

        // Shortest path from p1 to p3 should be [p1, p2, p3] with distance 1 (t1: non-silent, t2: silent)
        let path_result = shortest_path_cache.shortest_path(&p1.id, &p3.id);
        assert_eq!(
            path_result,
            Some(ShortestPathResult {
                path: vec![p1.id, p2.id, p3.id],
                distance: 1
            })
        );

        // Shortest path from p3 to p2 should be [p3, p1, p2] with distance 1 (t3: non-silent, t1: non-silent)
        let path_result = shortest_path_cache.shortest_path(&p3.id, &p2.id);
        assert_eq!(
            path_result,
            Some(ShortestPathResult {
                path: vec![p3.id, p1.id, p2.id],
                distance: 2
            })
        );
    }

    #[test]
    fn test_shortest_path_same_place() {
        let mut net = ObjectCentricPetriNet::new();

        // Add a single place.
        let p1 = net.add_place(Some("P1".to_string()), "Solo".to_string(), true, false);

        let petri_net = Arc::new(net);
        let shortest_path_cache = ShortestPathCache::new(Arc::clone(&petri_net));

        // Shortest path from p1 to p1 should be [p1] with distance 0
        let path_result = shortest_path_cache.shortest_path(&p1.id, &p1.id);
        assert_eq!(
            path_result,
            Some(ShortestPathResult {
                path: vec![p1.id],
                distance: 0
            })
        );
    }

    #[test]
    fn test_shortest_path_no_path() {
        let mut net = ObjectCentricPetriNet::new();

        // Add places with the same object_type.
        let p1 = net.add_place(Some("P1".to_string()), "Disconnected".to_string(), true, false);
        let p2 = net.add_place(Some("P2".to_string()), "Disconnected".to_string(), false, false);

        // Add a transition that doesn't connect p1 and p2.
        let t1 = net.add_transition("T1".to_string(), Some("Transition 1".to_string()), false);
        let t2 = net.add_transition("T2".to_string(), Some("Transition 2".to_string()), false);

        net.add_input_arc(p1.id, t1.id, false, 1);
        net.add_output_arc(t1.id, p1.id, false, 1);

        net.add_input_arc(p2.id, t2.id, false, 1);
        net.add_output_arc(t2.id, p2.id, false, 1);

        let petri_net = Arc::new(net);
        let shortest_path_cache = ShortestPathCache::new(Arc::clone(&petri_net));

        // No path from p1 to p2
        let path_result = shortest_path_cache.shortest_path(&p1.id, &p2.id);
        assert_eq!(path_result, None);
    }

    #[test]
    fn test_distances_between_all_places() {
        // Initialize your ObjectCentricPetriNet (ocpn) here

        let json_data =
            fs::read_to_string("./src/oc_align/test_data/bpi17/oc_petri_net.json").unwrap();
        let net = initialize_ocpn_from_json(&json_data);

        // After setting up your OCPN, continue with the test
        let petri_net = Arc::new(net);
        let shortest_path_cache = ShortestPathCache::new(Arc::clone(&petri_net));

        let places: Vec<&Place> = petri_net.places.values().collect();

        for from_place in &places {
            for to_place in &places {
                if from_place.id == to_place.id {
                    println!(
                        "Distance from '{}' to '{}': 0 (same place)",
                        from_place.name.as_deref().unwrap_or("Unnamed"),
                        to_place.name.as_deref().unwrap_or("Unnamed")
                    );
                    continue;
                }

                let path_result = shortest_path_cache.shortest_path(&from_place.id, &to_place.id);
                match path_result {
                    Some(result) => {
                        println!(
                            "Distance from '{}' to '{}': {}",
                            from_place.name.as_deref().unwrap_or("Unnamed"),
                            to_place.name.as_deref().unwrap_or("Unnamed"),
                            result.distance
                        );
                    }
                    None => {
                        println!(
                            "Distance from '{}' to '{}': No path",
                            from_place.name.as_deref().unwrap_or("Unnamed"),
                            to_place.name.as_deref().unwrap_or("Unnamed")
                        );
                    }
                }
            }
        }
    }
}