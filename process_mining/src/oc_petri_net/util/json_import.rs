use crate::oc_petri_net::oc_petri_net::ObjectCentricPetriNet;
use serde_json::Value;
use std::collections::HashMap;

pub fn initialize_ocpn_from_json(json_data: &str) -> ObjectCentricPetriNet {
    let parsed: Value = serde_json::from_str(json_data).unwrap();
    let mut ocpn = ObjectCentricPetriNet::new();

    // Create places
    let mut id_to_place = HashMap::new();
    if let Some(places) = parsed.get("places").and_then(|v| v.as_array()) {
        for place in places {
            let id = place.get("id").and_then(|v| v.as_u64()).unwrap();
            let name = place
                .get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let object_type = place
                .get("object_type")
                .and_then(|v| v.as_str())
                .unwrap()
                .to_string();
            let initial = place.get("initial").and_then(|v| v.as_bool()).unwrap();
            let final_place = place.get("final").and_then(|v| v.as_bool()).unwrap();

            let place_instance = ocpn.add_place(name, object_type, initial, final_place);
            id_to_place.insert(id, place_instance);
        }
    }

    // Create transitions
    let mut id_to_transition = HashMap::new();
    if let Some(transitions) = parsed.get("transitions").and_then(|v| v.as_array()) {
        for transition in transitions {
            let id = transition.get("id").and_then(|v| v.as_u64()).unwrap();
            let name = transition
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap()
                .to_string();
            let label = transition
                .get("label")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let silent = transition.get("silent").and_then(|v| v.as_bool()).unwrap();

            let transition_instance = ocpn.add_transition(label.clone().unwrap(), label, silent);
            id_to_transition.insert(id, transition_instance);
        }
    }

    // Create input arcs
    if let Some(input_arcs) = parsed.get("input_arcs").and_then(|v| v.as_array()) {
        for arc in input_arcs {
            let source_id = arc.get("source").and_then(|v| v.as_u64()).unwrap();
            let target_id = arc.get("target").and_then(|v| v.as_u64()).unwrap();
            let variable = arc.get("variable").and_then(|v| v.as_bool()).unwrap();
            let weight = arc.get("weight").and_then(|v| v.as_i64()).unwrap() as i32;

            if let (Some(source_place), Some(target_transition)) = (
                id_to_place.get(&source_id),
                id_to_transition.get(&target_id),
            ) {
                ocpn.add_input_arc(source_place.id, target_transition.id, variable, weight);
            }
        }
    }

    // Create output arcs
    if let Some(output_arcs) = parsed.get("output_arcs").and_then(|v| v.as_array()) {
        for arc in output_arcs {
            let source_id = arc.get("source").and_then(|v| v.as_u64()).unwrap();
            let target_id = arc.get("target").and_then(|v| v.as_u64()).unwrap();
            let variable = arc.get("variable").and_then(|v| v.as_bool()).unwrap();
            let weight = arc.get("weight").and_then(|v| v.as_i64()).unwrap() as i32;

            if let (Some(source_transition), Some(target_place)) = (
                id_to_transition.get(&source_id),
                id_to_place.get(&target_id),
            ) {
                ocpn.add_output_arc(source_transition.id, target_place.id, variable, weight);
            }
        }
    }

    ocpn
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_import_ocpn() {
        let json_data = fs::read_to_string("./src/oc_petri_net/util/oc_petri_net.json").unwrap();
        let ocpn = initialize_ocpn_from_json(&json_data);

        println!("{}", ocpn.get_initial_places().len());
        // Check that the petri net has the correct number of places and transitions
        assert_eq!(ocpn.places.len(), 3);
        assert_eq!(ocpn.transitions.len(), 2);
        

        // Check that the petri net has the correct number of input and output arcs
        assert_eq!(ocpn.input_arcs.len(), 3);
        assert_eq!(ocpn.output_arcs.len(), 3);
    }
}
