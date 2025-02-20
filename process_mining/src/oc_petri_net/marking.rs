use crate::oc_align::util::reachability_cache::ReachabilityCache;
use crate::oc_petri_net::oc_petri_net::{ObjectCentricPetriNet, Transition};
use crate::oc_petri_net::util::intersect_hashbag::intersect_hashbags;
use hashbag::HashBag;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use uuid::Uuid;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct OCToken {
    pub id: usize,
    //obj_id: str,
}

impl OCToken {
    pub fn new() -> Self {
        Self {
            id: COUNTER.fetch_add(1, Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Marking {
    petri_net: Arc<ObjectCentricPetriNet>,
    assignments: HashMap<Uuid, HashBag<OCToken>>,
}

impl Marking {
    pub fn new(petri_net: Arc<ObjectCentricPetriNet>) -> Self {
        Marking {
            petri_net: petri_net,
            assignments: HashMap::new(),
        }
    }

    pub fn add_initial_token_count(&mut self, place_id: &Uuid, count: usize) -> Vec<usize> {
        if !self
            .petri_net
            .get_place(place_id)
            .expect("Place not found")
            .initial
        {
            panic!("Place {} is not an initial place", place_id);
        }

        // create a new token count times and add it to a new hashbag
        let bag = self
            .assignments
            .entry(place_id.clone())
            .or_insert_with(|| HashBag::new());
        //initialize vec with count number of tokens
        let mut token_ids: Vec<usize> = Vec::with_capacity(count);
        for _ in 0..count {
            let token = OCToken::new();
            token_ids.push(token.id);
            bag.insert(token);
        }
        token_ids
    }

    pub fn add_initial_tokens(&mut self, place_id: &Uuid, tokens: &HashBag<OCToken>) {
        if !self
            .petri_net
            .get_place(place_id)
            .expect("Place not found")
            .initial
        {
            panic!("Place {} is not an initial place", place_id);
        }

        self._add_all_tokens_unsafe(place_id, tokens);
    }

    /// Adds a token to a place, regardless of the permissibility of the operation.
    /// It is strictly recommended to use add_initial_tokens instead
    pub fn _add_all_tokens_unsafe(&mut self, place_id: &Uuid, tokens: &HashBag<OCToken>) {
        let bag = self
            .assignments
            .entry(place_id.clone())
            .or_insert_with(|| HashBag::new());

        tokens.set_iter().for_each(|(token, count)| {
            bag.insert_many(token.clone(), count);
        });
    }

    pub fn add_token_unsafe(&mut self, place_id: Uuid, token: OCToken) {
        self.assignments
            .entry(place_id)
            .or_insert_with(|| HashBag::new())
            .insert(token);
    }

    /// Returns all possible firing combinations for the given transition.
    pub fn get_firing_combinations(&self, transition: &Transition) -> Vec<Binding> {
        let default: HashBag<OCToken> = HashBag::new();
        let mut input_place_map: HashMap<String, Vec<(&Uuid, &HashBag<OCToken>)>> = HashMap::new();

        let mut object_type_variable: HashMap<String, bool> = HashMap::new();

        // Group input places by object_type along with their required token counts
        for arc in transition.input_arcs.iter() {
            let place_id = &arc.source_place_id;
            let place = self.petri_net.get_place(place_id).expect("Place not found");

            let obj_type = place.object_type.clone();

            let bag = self.assignments.get(place_id).unwrap_or(&default);

            object_type_variable.insert(obj_type.clone(), arc.variable);

            if (bag.len() == 0) {
                return vec![];
            }

            input_place_map
                .entry(obj_type)
                .or_insert_with(Vec::new)
                .push((place_id, bag));
        }

        // One for obj_type, one for place, one for all variable arc combinations
        let mut obj_type_tokens: Vec<Vec<ObjectBindingInfo>> = Vec::new();

        let mut common_tokens_per_type: HashMap<String, HashBag<OCToken>> = HashMap::new();

        // first assert all obj types required for the transition have common tokens
        for (_obj_type, places) in input_place_map.iter() {
            let common_tokens =
                intersect_hashbags(&*places.iter().map(|(_, bag)| *bag).collect::<Vec<_>>());

            if common_tokens.len() == 0 {
                return vec![];
            }

            common_tokens_per_type.insert(_obj_type.clone(), common_tokens);
        }

        // For each object type, find tokens that satisfy all input places
        for (_obj_type, places) in input_place_map.iter() {
            // if (transition.name == "place order") {
            // //     println!("-------------------");
            // //     println!("transition: {}", transition.name);
            //      println!("obj_type: {}", _obj_type);
            //      println!("places: {:?}", places);
            // //     println!("-------------------");
            // }
            let common_tokens = common_tokens_per_type.get(_obj_type).expect("Common tokens not found");

            let firings = {
                //Normal places
                if !object_type_variable.get(_obj_type).unwrap() {
                    common_tokens
                        .set_iter()
                        .map(|(token, token_count)| ObjectBindingInfo {
                            object_type: _obj_type.clone(),
                            tokens: vec![token.clone()],
                            place_bindings: places
                                .iter()
                                .map(|(place_id, _)| PlaceBinding {
                                    count: 1,
                                    consumed: vec![token.clone()],
                                    place_id: *place_id.clone(),
                                })
                                .collect(),
                        })
                        .collect()
                } else {
                    //Variable places
                    // compute possible combinations of tokens for the variable arc to take, at least one token must be taken
                    let mut power_sets = power_set(&common_tokens);
                    if (power_sets.len() == 0) {
                        return vec![];
                    }
                    power_sets.swap_remove(0);

                    power_sets
                        .into_iter()
                        .map(|token_set| ObjectBindingInfo {
                            object_type: _obj_type.clone(),
                            tokens: token_set.iter().map(|token| token.clone()).collect(),
                            place_bindings: places
                                .iter()
                                .map(|(place_id, _)| PlaceBinding {
                                    count: 1,
                                    consumed: token_set.iter().map(|token| token.clone()).collect(),
                                    place_id: *place_id.clone(),
                                })
                                .collect(),
                        })
                        .collect()
                }
            };

            obj_type_tokens.push(firings);
        }

        // Compute cartesian product of tokens across all object types
        if obj_type_tokens.is_empty() {
            return vec![];
        }

        let product = cartesian_product_iter(obj_type_tokens);

        // Convert each product into a firing combination map

        product
            .into_iter()
            .map(|combination| Binding::from_combinations(transition.id, combination))
            .collect()
    }

    pub fn fire_transition(&mut self, transition: &Transition, binding: &Binding) {
        for obj_binding in binding.object_binding_info.values() {
            // Remove Tokens from input places
            for place_binding in obj_binding.place_bindings.iter() {
                let bag = self
                    .assignments
                    .get_mut(&place_binding.place_id)
                    .expect("Place not found");
                for token in place_binding.consumed.iter() {
                    // TODO make clear this will not panic if the token is not found or not enough tokens are found
                    bag.remove_up_to(&token, place_binding.count);
                }
            }
        }
        // Add tokens to output places
        for arc in transition.output_arcs.iter() {
            let bag = self
                .assignments
                .entry(arc.target_place_id)
                .or_insert_with(|| HashBag::new());
            let place = self.petri_net.get_place(&arc.target_place_id).unwrap();
            for token in binding
                .object_binding_info
                .get(&place.object_type)
                .unwrap()
                .tokens
                .iter()
            {
                bag.insert(token.clone());
            }
        }
    }

    /// Checks if the transition is enabled by verifying if there is at least one firing combination.
    pub fn is_enabled(&self, transition: &Transition) -> bool {
        !self.get_firing_combinations(transition).is_empty()
    }

    pub fn is_final(&self) -> bool {
        self.assignments.iter().all(|(place_id, tokens)| {
            tokens.is_empty() || self.petri_net.get_place(place_id).unwrap().final_place
        })
    }

    pub fn is_final_has_tokens(&self) -> bool {
        let has_tokens = self.assignments.values().any(|tokens| !tokens.is_empty());
        has_tokens
            && self.assignments.iter().all(|(place_id, tokens)| {
                tokens.is_empty() || self.petri_net.get_place(place_id).unwrap().final_place
            })
    }

    pub fn get_initial_counts_per_type(&self) -> HashMap<String, usize> {
        let mut initial_counts = HashMap::new();
        for (place_id, tokens) in self.assignments.iter() {
            let place = self.petri_net.get_place(place_id).unwrap();
            if place.initial {
                let obj_type = place.object_type.clone();
                let count = tokens.len();
                initial_counts.insert(obj_type, count);
            }
        }
        initial_counts
    }

    pub fn has_dead_places(
        &self,
        transition_enabled: &HashMap<Uuid, bool>,
        reachability_cache: &ReachabilityCache,
    ) -> Vec<Uuid> {
        let mut dead_places = Vec::new();

        // Collect all non-final places that have tokens in the current marking
        let places_with_tokens: Vec<Uuid> = self
            .assignments
            .iter()
            .filter(|(_, bag)| !bag.is_empty())
            .filter(|(place_id, _)| {
                let place = self.petri_net.get_place(place_id).unwrap();
                !place.final_place
            })
            .map(|(place_id, _)| *place_id)
            .collect();

        // Iterate through all candidate dead places
        for &place_id in &places_with_tokens {
            let place = match self.petri_net.get_place(&place_id) {
                Some(p) => p,
                None => panic!(
                    "Place with ID {:?} does not exist in the Petri net.",
                    place_id
                ),
            };

            // Already filtered out final places, but double-check
            if place.final_place {
                continue;
            }

            // Check if any transition away from this place is enabled
            let mut has_enabled_transition = false;

            for arc in &place.output_arcs {
                let transition_id = arc.target_transition_id;
                if let Some(&is_enabled) = transition_enabled.get(&transition_id) {
                    if is_enabled {
                        has_enabled_transition = true;
                        break;
                    }
                }
            }

            // If there's at least one enabled transition, it's not a dead place
            if has_enabled_transition {
                continue;
            }

            // Now, verify two conditions:
            // 1. The dead place is not reachable from any other place with tokens.
            // 2. The transitions connected to the dead place cannot be enabled by adding tokens
            //    to other input places from reachable token-holding places.

            // Condition 1: Check if the dead place is reachable from any other place with tokens
            let is_reachable = places_with_tokens.iter().any(|&other_place_id| {
                if other_place_id == place_id {
                    // Skip the same place
                    false
                } else {
                    reachability_cache.is_reachable(&other_place_id, &place_id)
                }
            });

            // If the place is reachable from another place with tokens, it might not be dead
            if is_reachable {
                continue;
            }

            // Condition 2: For each transition connected to the dead place, ensure it cannot be
            // enabled by adding tokens to other input places.
            let mut can_enable_transition = false;

            for arc in &place.output_arcs {
                let transition_id = arc.target_transition_id;
                let transition = match self.petri_net.get_transition(&transition_id) {
                    Some(t) => t,
                    None => continue, // Transition does not exist, skip
                };

                // Collect all input places for this transition excluding the dead place
                let other_input_places: Vec<Uuid> = transition
                    .input_arcs
                    .iter()
                    .filter_map(|input_arc| {
                        if input_arc.source_place_id != place_id {
                            Some(input_arc.source_place_id)
                        } else {
                            None
                        }
                    })
                    .collect();

                // If there are no other input places, the transition cannot be enabled without the dead place
                if other_input_places.is_empty() {
                    continue;
                }

                // Check if all other input places are unreachable, meaning tokens cannot be added
                // to them from the current marking.
                let other_inputs_reachable = other_input_places.iter().any(|&input_id| {
                    // A place can have tokens added if it's reachable from any current token-holding place
                    self.assignments.iter().any(|(current_place_id, _)| {
                        // Skip the dead place itself
                        current_place_id != &place_id
                            && reachability_cache.is_reachable(current_place_id, &input_id)
                    })
                });

                // If at least one other input place is reachable, the transition can potentially be enabled
                if other_inputs_reachable {
                    can_enable_transition = true;
                    break; // No need to check other transitions, place is not dead
                }
            }

            // If none of the connected transitions can be enabled via other input places, it's dead
            if !can_enable_transition {
                dead_places.push(place_id.clone());
            }
        }

        dead_places
    }
}

impl Binding {
    fn from_combinations(transition_id: Uuid, combinations: Vec<ObjectBindingInfo>) -> Self {
        Binding {
            object_binding_info: combinations
                .iter()
                .map(|c| (c.object_type.clone(), c.clone()))
                .collect(),
            transition_id,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlaceBinding {
    pub place_id: Uuid,
    pub consumed: Vec<OCToken>,
    pub count: usize,
}

#[derive(Debug, Clone)]
pub struct ObjectBindingInfo {
    pub object_type: String,
    pub tokens: Vec<OCToken>,
    pub place_bindings: Vec<PlaceBinding>,
}

impl PartialEq for ObjectBindingInfo {
    fn eq(&self, other: &Self) -> bool {
        self.tokens == other.tokens
    }
}

// impl Hash for ObjectBindingInfo {
//     fn hash<H: Hasher>(&self, state: &mut H) {
//         self.tokens.hash(state);
//     }
// }
// 
// impl Hash for Binding {
//     fn hash<H: Hasher>(&self, state: &mut H) {
//         self.transition_id.hash(state);
//         self.object_binding_info.hash(state);
//     }
// }

#[derive(Debug, Clone)]
pub struct Binding {
    /// Tokens to take out of the place
    pub transition_id: Uuid,
    pub object_binding_info: HashMap<String, ObjectBindingInfo>,
}

impl PartialEq for Binding {
    fn eq(&self, other: &Self) -> bool {
        self.transition_id == other.transition_id
            && self.object_binding_info == other.object_binding_info
    }
}

impl std::fmt::Display for Binding {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let object_names: Vec<String> = self
            .object_binding_info
            .values()
            .map(|binding_info| {
                let object_name = binding_info.object_type.clone();
                let token_count = binding_info.tokens.len();
                format!("{}: {}", object_name, token_count)
            })
            .collect();
        write!(f, "{}", object_names.join(", "))
    }
}

fn cartesian_product<T>(inputs: Vec<Vec<&T>>) -> Vec<Vec<&T>> {
    inputs.into_iter().fold(vec![Vec::new()], |acc, pool| {
        acc.into_iter()
            .flat_map(|combination| {
                pool.iter().map(move |&item| {
                    let mut new_combination = combination.clone();
                    new_combination.push(item);
                    new_combination
                })
            })
            .collect()
    })
}

fn cartesian_product_iter<T: Clone>(inputs: Vec<Vec<T>>) -> Vec<Vec<T>> {
    inputs.into_iter().fold(vec![Vec::new()], |acc, pool| {
        acc.into_iter()
            .flat_map(|combination| {
                pool.iter().map(move |item| {
                    let mut new_combination = combination.clone();
                    new_combination.push(item.clone());
                    new_combination
                })
            })
            .collect()
    })
}
fn power_multiset<T: Clone + std::hash::Hash + Eq>(bag: &HashBag<T>) -> Vec<HashBag<T>> {
    let mut bag = bag.clone();
    let distinct_items: HashMap<&T, usize> = bag.set_iter().collect();

    // Start with an empty multiset
    let mut power_multisets = vec![HashBag::new()];

    for (item, count) in distinct_items {
        let current_len = power_multisets.len();

        // Iterate over current power sets and extend them based on the multiplicity of the current item
        for times in 1..=count {
            for i in 0..current_len {
                let mut new_subset = power_multisets[i].clone();
                new_subset.extend(std::iter::repeat(item.clone()).take(times));
                power_multisets.push(new_subset);
            }
        }
    }

    // remove the empty set
    power_multisets.swap_remove(0);

    power_multisets
}

fn power_set<T: Clone + std::hash::Hash + Eq>(bag: &HashBag<T>) -> Vec<HashSet<T>> {
    let unique_items: Vec<T> = bag.set_iter().map(|(item, _)| item.clone()).collect();
    let num_subsets = 1 << unique_items.len(); // 2^n subsets

    let mut power_sets = Vec::with_capacity(num_subsets);

    for i in 0..num_subsets {
        let mut subset = HashSet::new();
        for (j, item) in unique_items.iter().enumerate() {
            if (i & (1 << j)) != 0 {
                subset.insert(item.clone());
            }
        }
        power_sets.push(subset);
    }

    power_sets
}
