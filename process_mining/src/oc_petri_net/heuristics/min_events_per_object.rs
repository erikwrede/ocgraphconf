// File: process_mining/src/oc_align/heuristic_transition_lower_bound.rs
use crate::oc_petri_net::oc_petri_net::{ObjectCentricPetriNet, Place};
use crate::type_storage::{EventType, ObjectType};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// Computes a lower bound of transitions (by intersection)
/// that are hit on every acyclic path (i.e. “must‐occur” events)
/// for each object type.
/// first bool: true if is solo event - can be estimated with cost 2
/// second bool: true if can not be first event (none of the places are initial) - can be estimated with cost 2
pub fn compute_must_transitions(
    net: &ObjectCentricPetriNet,
) -> HashMap<ObjectType, HashSet<(EventType, bool, bool)>> {
    // mapping: object type -> vector of transition sets from each complete path
    let mut paths_by_type: HashMap<ObjectType, Vec<HashSet<Uuid>>> = HashMap::new();

    // Get initial places and start DFS for each one by object type.
    for place in net.places.values().filter(|p| p.initial) {
        let obj_type = place.oc_object_type.clone();
        let entry = paths_by_type
            .entry(obj_type.clone())
            .or_insert_with(Vec::new);
        // DFS collects transitions (using an empty set start)
        dfs_collect_paths(
            net,
            place.id,
            &obj_type,
            &mut HashSet::new(),
            &mut HashSet::new(),
            entry,
        );
    }

    // Compute the intersection over all path sets per object type.
    let mut result: HashMap<ObjectType, HashSet<(EventType,bool, bool)>> = HashMap::new();
    for (obj_type, path_sets) in paths_by_type {
        if let Some(first_set) = path_sets.first() {
            let mut inter = first_set.clone();
            for ps in path_sets.iter().skip(1) {
                inter = inter.intersection(ps).cloned().collect();
            }

            let events = inter
                .iter()
                .map(|t| {
                    let transition = net.get_transition(t).unwrap();
                    let input_places : Vec<&Place> = transition.input_arcs.iter().map(|arc|
                        net.get_place(&arc.source_place_id).unwrap()
                    ).collect();
                    let solo_transition =
                        transition.input_arcs.iter().any(|arc| {
                            let place = net.get_place(&arc.source_place_id).unwrap();
                            place.oc_object_type != obj_type || arc.variable
                        });
                    let can_be_first_event = input_places.iter().all(|place| place.initial);
                    (
                        transition.event_type.clone(),
                        !solo_transition,
                        !can_be_first_event
                    )
                })
                .collect();

            result.insert(obj_type, events);
        } else {
            result.insert(obj_type, HashSet::new());
        }
    }
    result
}

/// DFS helper
/// Traverses from the current place, adds a transition when following an arc from a place
/// to a transition and records the set when no further same type places can be reached.
/// visited_places is used to avoid loops.
fn dfs_collect_paths(
    net: &ObjectCentricPetriNet,
    current_place_id: Uuid,
    obj_type: &ObjectType,
    current_transitions: &mut HashSet<Uuid>,
    visited_places: &mut HashSet<Uuid>,
    collected_paths: &mut Vec<HashSet<Uuid>>,
) {
    visited_places.insert(current_place_id);

    // Get outgoing connections (each output arc from the place points to a transition).
    let mut has_successor = false;
    if let Some(current_place) = net.get_place(&current_place_id) {
        for arc in current_place.output_arcs.iter() {
            let trans_id = arc.target_transition_id;
            // Only traverse a transition once per path.
            if current_transitions.contains(&trans_id) {
                continue;
            }
            // Ensure that this transition leads to next places of the same object type.
            if let Some(trans) = net.get_transition(&trans_id) {
                // Prepare the new transition set including this transition.
                let mut new_transitions = current_transitions.clone();
                new_transitions.insert(trans_id);

                for out_arc in trans.output_arcs.iter() {
                    let next_place_id = out_arc.target_place_id;
                    if let Some(next_place) = net.get_place(&next_place_id) {
                        if next_place.oc_object_type == *obj_type
                            && !visited_places.contains(&next_place_id)
                        {
                            has_successor = true;
                            // Clone visited set for each branch to avoid cross contamination.
                            let mut new_visited = visited_places.clone();
                            dfs_collect_paths(
                                net,
                                next_place_id,
                                obj_type,
                                &mut new_transitions,
                                &mut new_visited,
                                collected_paths,
                            );
                        }
                    }
                }
            }
        }
    }

    // If no valid successor, record the current path.
    if !has_successor {
        collected_paths.push(current_transitions.clone());
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use crate::oc_petri_net::heuristics::min_events_per_object::compute_must_transitions;
    use crate::oc_petri_net::initialize_ocpn_from_json;
    use crate::type_storage::{EventType, ObjectType};
    use std::fs;

    #[test]
    fn large_petri_net() {
        let json_data =
            fs::read_to_string("./src/oc_align/test_data/bpi17/oc_petri_net.json").unwrap();
        let ocpn = initialize_ocpn_from_json(&json_data);

        let transitions = compute_must_transitions(&ocpn);
        // expected: offer: ["Create offer", "Send (mail and online)"]
        // application: ["Create application", "Validate", "Accept"]

        let ot_offer: ObjectType = "offer".into();
        let et_create_offer: EventType = "Create offer".into();
        let et_send: EventType = "Send (mail and online)".into();

        let ot_application: ObjectType = "application".into();
        let et_create_application: EventType = "Create application".into();
        let et_validate: EventType = "Validate".into();
        let et_accept: EventType = "Accept".into();

        assert_eq!(
            transitions.get(&ot_offer).unwrap(),
            &HashSet::from([(et_create_offer, false, true ), (et_send,true, true)])
        );

        assert_eq!(
            transitions.get(&ot_application).unwrap(),
            &HashSet::from([(et_create_application, true, false), (et_validate, true, true), (et_accept, true, true)])
        );
    }
}
