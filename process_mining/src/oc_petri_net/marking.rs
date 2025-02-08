use crate::oc_petri_net::oc_petri_net::{InputArc, ObjectCentricPetriNet, Transition};
use crate::oc_petri_net::util::intersect_hashbag::intersect_hashbags;
use hashbag::HashBag;
use std::collections::{HashMap, HashSet};
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
    pub fn new(petri_net: ObjectCentricPetriNet) -> Self {
        Marking {
            petri_net: petri_net.into(),
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
        let arcs_to_place: HashMap<Uuid, Vec<&InputArc>> =
            transition
                .input_arcs
                .iter()
                .fold(HashMap::new(), |mut acc, arc| {
                    acc.entry(arc.source_place_id)
                        .or_insert_with(Vec::new)
                        .push(arc);
                    acc
                });

        let default: HashBag<OCToken> = HashBag::new();
        let mut input_place_map: HashMap<String, Vec<(&Uuid, HashBag<OCToken>, usize)>> =
            HashMap::new();

        let mut object_type_variable: HashMap<String, bool> = HashMap::new();

        // Group input places by object_type along with their required token counts
        for (place_id, arcs) in arcs_to_place.iter() {
            let place = self.petri_net.get_place(place_id).expect("Place not found");
            let obj_type = place.object_type.clone();
            let consuming_arc_count = arcs.len();
            let bag = self.assignments.get(place_id).unwrap_or(&default);

            // all arcs to place must either be variable or non-variable
            // if this is not the case, panic
            let var_arc_count = arcs.iter().filter(|arc| arc.variable).count();
            let non_var_arc_count = consuming_arc_count - var_arc_count;

            let is_variable = var_arc_count > 0;

            if is_variable && non_var_arc_count > 0 {
                panic!("Petri-Net isn't well-formed! All arcs to a place must either be variable or non-variable");
            }

            // now insert the object type and if it is variable into the map, panic if that is incompatible with existing info in the map
            if let Some(&is_var) = object_type_variable.get(&obj_type) {
                if is_var != (is_variable) {
                    panic!("Petri-Net isn't well-formed! All arcs to a place must either be variable or non-variable");
                }
            } else {
                object_type_variable.insert(obj_type.clone(), is_variable);
            }

            // Filter the bag to retain only tokens with a count >= consuming_arc_count
            let mut filtered_bag = bag.clone();
            filtered_bag.retain(|_, count| {
                if count >= consuming_arc_count {
                    return count;
                }
                return 0;
            });

            input_place_map
                .entry(obj_type)
                .or_insert_with(Vec::new)
                .push((place_id, filtered_bag, consuming_arc_count));
        }
        // One for obj_type, one for place, one for all variable arc combinations
        let mut obj_type_tokens: Vec<Vec<ObjectBindingInfo>> = Vec::new();

        // For each object type, find tokens that satisfy all input places
        for (_obj_type, places) in input_place_map.iter() {
            let common_tokens =
                intersect_hashbags(&*places.iter().map(|(_, bag, _)| bag).collect::<Vec<_>>());

            // If there are no common tokens, we can't fire the transition
            if (common_tokens.len() == 0) {
                return vec![];
            }

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
                                .map(|(place_id, _, consuming_arc_count)| PlaceBinding {
                                    count: *consuming_arc_count,
                                    consumed: vec![token.clone(); *consuming_arc_count],
                                    place_id: *place_id.clone(),
                                })
                                .collect(),
                        })
                        .collect()
                } else {
                    //Variable places
                    // compute possible combinations of tokens for the variable arc to take, at least one token must be taken
                    let mut power_sets = power_set(&common_tokens);
                    power_sets.swap_remove(0);

                    power_sets
                        .into_iter()
                        .map(|token_set| ObjectBindingInfo {
                            object_type: _obj_type.clone(),
                            tokens: token_set.iter().map(|token| token.clone()).collect(),
                            place_bindings: places
                                .iter()
                                .map(|(place_id, _, consuming_arc_count)| PlaceBinding {
                                    count: *consuming_arc_count,
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
            for obj_binding in binding.object_binding_info.values() {
                for token in obj_binding.tokens.iter() {
                    bag.insert(token.clone());
                }
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
        has_tokens && self.assignments.iter().all(|(place_id, tokens)| {
            tokens.is_empty() || self.petri_net.get_place(place_id).unwrap().final_place
        })
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

#[derive(Debug, Clone)]
pub struct Binding {
    /// Tokens to take out of the place
    pub transition_id: Uuid,
    pub object_binding_info: HashMap<String, ObjectBindingInfo>,
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
