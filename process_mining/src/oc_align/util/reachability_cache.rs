use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use uuid::Uuid;
use crate::oc_petri_net::oc_petri_net::ObjectCentricPetriNet;

/// Struct to cache reachability information between places in the Petri net.
#[derive(Debug)]
pub struct ReachabilityCache {
    /// Reference to the Petri net.
    petri_net: Arc<ObjectCentricPetriNet>,
    /// Cache storing reachability results. The key is a tuple of (from_place_id, to_place_id).
    cache: Mutex<HashMap<(Uuid, Uuid), bool>>,
}

impl ReachabilityCache {
    /// Creates a new ReachabilityCache associated with a given Petri net.
    pub fn new(petri_net: Arc<ObjectCentricPetriNet>) -> Self {
        ReachabilityCache {
            petri_net,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Checks if `to_place_id` is reachable from `from_place_id`.
    /// Utilizes caching to store and retrieve previous computations.
    pub fn is_reachable(&self, from_place_id: &Uuid, to_place_id: &Uuid) -> bool {
        // If both places are the same, it's trivially reachable.
        if from_place_id == to_place_id {
            return true;
        }

        // Acquire the lock to access the cache.
        {
            let cache_guard = self.cache.lock().unwrap();
            if let Some(&result) = cache_guard.get(&(*from_place_id, *to_place_id)) {
                return result;
            }
        }

        // Perform BFS to check reachability.
        let result = self.bfs_reachable(from_place_id, to_place_id);

        // Store the result in the cache.
        let mut cache_guard = self.cache.lock().unwrap();
        cache_guard.insert((*from_place_id, *to_place_id), result);

        result
    }

    /// Performs a BFS to determine if `to_place_id` is reachable from `from_place_id`.
    fn bfs_reachable(&self, from_place_id: &Uuid, to_place_id: &Uuid) -> bool {
        let mut visited_places = HashSet::new();
        let mut visited_transitions = HashSet::new();
        let mut queue = VecDeque::new();

        // Start from the source place.
        queue.push_back(from_place_id.clone());
        visited_places.insert(from_place_id.clone());

        while let Some(current_place_id) = queue.pop_front() {
            // Get the place from the Petri net.
            let place = match self.petri_net.get_place(&current_place_id) {
                Some(p) => p,
                None => continue, // If place doesn't exist, skip.
            };

            // Iterate over all outgoing input arcs (i.e., transitions connected to this place).
            for input_arc in &place.output_arcs {
                // Corrected: Use `target_transition_id` instead of `source_transition_id`
                let transition_id = input_arc.target_transition_id;

                // Skip if we've already visited this transition.
                if !visited_transitions.insert(transition_id) {
                    continue;
                }

                // Get the transition.
                let transition = match self.petri_net.get_transition(&transition_id) {
                    Some(t) => t,
                    None => continue, // If transition doesn't exist, skip.
                };

                // Iterate over all output arcs of the transition to reach new places.
                for output_arc in &transition.output_arcs {
                    let next_place_id = output_arc.target_place_id;

                    // If we've reached the target place, return true.
                    if &next_place_id == to_place_id {
                        return true;
                    }

                    // If not visited, add to the queue.
                    if visited_places.insert(next_place_id.clone()) {
                        queue.push_back(next_place_id.clone());
                    }
                }
            }
        }

        // If BFS completes without finding the target, it's not reachable.
        false
    }
}

#[cfg(test)]
mod reachability_tests {
    use super::*;

    #[test]
    fn test_reachability_simple() {
        let mut net = ObjectCentricPetriNet::new();

        // Add places
        let p1 = net.add_place(Some("P1".to_string()), "A".to_string(), true, false);
        let p2 = net.add_place(Some("P2".to_string()), "B".to_string(), false, true);
        let p3 = net.add_place(Some("P3".to_string()), "C".to_string(), false, true);

        // Add transitions
        let t1 = net.add_transition("T1".to_string(), Some("Transition 1".to_string()), false);
        let t2 = net.add_transition("T2".to_string(), None, true);

        // Add arcs
        let arc1 = net.add_input_arc(p1.id, t1.id, false, 1);
        let arc2 = net.add_output_arc(t1.id, p2.id, false, 1);
        let arc3 = net.add_input_arc(p2.id, t2.id, true, 2);
        let arc4 = net.add_output_arc(t2.id, p3.id, true, 2);

        // Additional arc for complexity: p1 -> t2
        let arc_a = net.add_input_arc(p1.id, t2.id, false, 1);

        let petri_net = Arc::new(net);
        let reachability_cache = ReachabilityCache::new(Arc::clone(&petri_net));

        // p3 is reachable from p1
        assert!(reachability_cache.is_reachable(&p1.id, &p3.id));

        // p2 is reachable from p1
        assert!(reachability_cache.is_reachable(&p1.id, &p2.id));

        // p1 is reachable from p1
        assert!(reachability_cache.is_reachable(&p1.id, &p1.id));

        // p1 is not reachable from p3
        assert!(!reachability_cache.is_reachable(&p3.id, &p1.id));

        // p3 is reachable from p2 (corrected expectation)
        assert!(reachability_cache.is_reachable(&p2.id, &p3.id));
    }

    #[test]
    fn test_reachability_cycle() {
        let mut net = ObjectCentricPetriNet::new();

        // Create places
        let p1 = net.add_place(Some("P1".to_string()), "A".to_string(), true, false);
        let p2 = net.add_place(Some("P2".to_string()), "B".to_string(), false, false);

        // Create transitions
        let t1 = net.add_transition("T1".to_string(), None, false);
        let t2 = net.add_transition("T2".to_string(), None, false);

        // Connect p1 <-> t1 <-> p2 <-> t2 <-> p1 (cycle)
        net.add_input_arc(p1.id, t1.id, false, 1);
        net.add_output_arc(t1.id, p2.id, false, 1);
        net.add_input_arc(p2.id, t2.id, false, 1);
        net.add_output_arc(t2.id, p1.id, false, 1);

        let petri_net = Arc::new(net);
        let reachability_cache = ReachabilityCache::new(Arc::clone(&petri_net));

        // p2 is reachable from p1
        assert!(reachability_cache.is_reachable(&p1.id, &p2.id));

        // p1 is reachable from p2
        assert!(reachability_cache.is_reachable(&p2.id, &p1.id));

        // p1 is reachable from itself
        assert!(reachability_cache.is_reachable(&p1.id, &p1.id));
    }

    #[test]
    fn test_reachability_disconnected() {
        let mut net = ObjectCentricPetriNet::new();

        // Create places
        let p1 = net.add_place(Some("P1".to_string()), "A".to_string(), true, false);
        let p2 = net.add_place(Some("P2".to_string()), "B".to_string(), true, false);
        let p3 = net.add_place(Some("P3".to_string()), "C".to_string(), false, true);

        // Create transitions
        let t1 = net.add_transition("T1".to_string(), None, false);

        // Connect p1 -> t1 -> p3
        net.add_input_arc(p1.id, t1.id, false, 1);
        net.add_output_arc(t1.id, p3.id, false, 1);

        let petri_net = Arc::new(net);
        let reachability_cache = ReachabilityCache::new(Arc::clone(&petri_net));

        // p3 is reachable from p1
        assert!(reachability_cache.is_reachable(&p1.id, &p3.id));

        // p3 is not reachable from p2
        assert!(!reachability_cache.is_reachable(&p2.id, &p3.id));

        // p2 is not reachable from p1
        assert!(!reachability_cache.is_reachable(&p1.id, &p2.id));
    }
}