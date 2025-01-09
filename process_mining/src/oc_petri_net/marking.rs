use crate::oc_petri_net::oc_petri_net::{InputArc, ObjectCentricPetriNet, Transition};
use crate::oc_petri_net::util::intersect_hashbag::intersect_hashbags;
use hashbag::HashBag;
use std::collections::HashMap;
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
        let mut input_place_map: HashMap<String, Vec<(&Uuid, HashBag<OCToken>, usize, usize)>> =
            HashMap::new();

        let mut max_var_arcs_per_object_type: HashMap<String, usize> = HashMap::new();
        let mut max_non_var_arcs_on_var_arc_place_per_object_type: HashMap<String, usize> =
            HashMap::new();
        // Group input places by object_type along with their required token counts
        for (place_id, arcs) in arcs_to_place.iter() {
            let place = self.petri_net.get_place(place_id).expect("Place not found");
            let obj_type = place.object_type.clone();
            let consuming_arc_count = arcs.len();
            let bag = self.assignments.get(place_id).unwrap_or(&default);

            // Filter the bag to retain only tokens with a count >= consuming_arc_count
            let mut filtered_bag = bag.clone();
            filtered_bag.retain(|_, count| {
                if count >= consuming_arc_count {
                    return count;
                }
                return 0;
            });

            let var_arc_count = arcs.iter().filter(|arc| arc.variable).count();
            let non_var_arc_count = consuming_arc_count - var_arc_count;

            let max_var_arcs = max_var_arcs_per_object_type
                .entry(obj_type.clone())
                .or_insert(0);

            if var_arc_count > *max_var_arcs {
                *max_var_arcs = var_arc_count;
            }

            if (var_arc_count > 0) {
                let max_non_var_arcs = max_non_var_arcs_on_var_arc_place_per_object_type
                    .entry(obj_type.clone())
                    .or_insert(0);

                if non_var_arc_count > *max_non_var_arcs {
                    *max_non_var_arcs = non_var_arc_count;
                }
            }

            input_place_map
                .entry(obj_type)
                .or_insert_with(Vec::new)
                .push((place_id, filtered_bag, consuming_arc_count, var_arc_count));
        }
        // One for obj_type, one for place, one for all variable arc combinations
        let mut obj_type_tokens: Vec<Vec<Vec<PlaceBindingInfo>>> = Vec::new();

        // For each object type, find tokens that satisfy all input places
        for (_obj_type, places) in input_place_map.iter() {
            let common_tokens =
                intersect_hashbags(&*places.iter().map(|(_, bag, _, _)| bag).collect::<Vec<_>>());

            let var_arc_places_common_tokens = {
                let var_arc_bags: Vec<HashBag<OCToken>> = places
                    .iter()
                    // Filter for places with at least one variable arc
                    .filter(|(_, _, _, v_a_c)| *v_a_c > 0)
                    // Do this later, as we only know which tokens to subtract when we select the token for firing
                    /*.map(|(_, bag, req, var_arc_count)| {
                        let mut cloned_bag = bag.clone();
                        // FIXME check correctness!! subtract var arc count from req
                        // i think this should be consuming_arc_count
                        cloned_bag.retain(|_, count| count.saturating_sub(req - var_arc_count));
                        cloned_bag
                    })*/
                    .map(|(_, bag, _, _)| bag.clone())
                    .collect();
                let var_arc_bag_refs: Vec<&HashBag<OCToken>> = var_arc_bags.iter().collect();
                intersect_hashbags(&var_arc_bag_refs)
            };

            // If there are no common tokens, we can't fire the transition
            if (common_tokens.len() == 0) {
                return vec![];
            }

            let firings = common_tokens
                .set_iter()
                .map(|(token, token_count)| {
                    // per token, get the variable arc info
                    let max_non_var_arcs = max_non_var_arcs_on_var_arc_place_per_object_type
                        .get(_obj_type)
                        .unwrap_or(&0);
                    let max_var_arcs = max_var_arcs_per_object_type.get(_obj_type).unwrap_or(&0);

                    // now, given the maximum number of arcs consuming tokens from a place connected via variable arcs
                    // we can subtract this number from the available tokens of the selected token id for each place

                    let mut intermediate_var_bag = var_arc_places_common_tokens.clone();
                    intermediate_var_bag.remove_up_to(token, max_non_var_arcs.clone());

                    // then, given the maximum number of variable arcs on a place, we can divide the available tokens by this number
                    // to get the number of possible combinations
                    // make sure the division is floored
                    intermediate_var_bag.retain(|_, count| count / max_var_arcs);
                    let var_arc_firing_combinations = power_multiset(&intermediate_var_bag);
                    // now, for any arbitrary token multiset from the intermediate bag plus the selected common token, we can build a place binding info and the
                    places
                        .iter()
                        .map(|(place_id, _, req, var_arc_count)| {
                            if (*var_arc_count == 0) {
                                let mut token_bag = HashBag::new();
                                token_bag.insert_many(token.clone(), *req);

                                let mut list_size = var_arc_firing_combinations.len();
                                if (list_size == 0) {
                                    list_size = 1;
                                }

                                let mut list = Vec::with_capacity(list_size);
                                for i in 0..list_size {
                                    list.push(PlaceBindingInfo {
                                        consumed: token_bag.clone(),
                                        place_id: *place_id.clone(),
                                    });
                                }
                                return list;
                            }
                            let result = var_arc_firing_combinations
                                .iter()
                                .map(|combination| {
                                    let mut token_bag = HashBag::new();

                                    // we are not taking the var arcs like this
                                    token_bag.insert_many(token.clone(), *req - var_arc_count);

                                    combination.set_iter().for_each(|(token, count)| {
                                        token_bag.insert_many(token.clone(), count);
                                    });

                                    return PlaceBindingInfo {
                                        consumed: token_bag,
                                        place_id: *place_id.clone(),
                                    };
                                })
                                .collect();

                            result
                        })
                        .collect()
                })
                .collect();
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

    /// Checks if the transition is enabled by verifying if there is at least one firing combination.
    pub fn is_enabled(&self, transition: &Transition) -> bool {
        !self.get_firing_combinations(transition).is_empty()
    }

    pub fn is_final(&self) -> bool {
        self.assignments.iter().all(|(place_id, tokens)| {
            tokens.is_empty() || self.petri_net.get_place(place_id).unwrap().final_place
        })
    }
}

impl Binding {
    fn from_combinations(
        transition_id: Uuid,
        combinations: Vec<Vec<PlaceBindingInfo>>,
        var_arc_counts: Vec,
    ) -> Self {
        Binding {
            tokens: combinations
                .iter()
                .fold(HashMap::new(), |mut acc, bindings| {
                    bindings.into_iter().for_each(|binding| {
                        acc.insert(binding.place_id, binding.clone()); // fixme clone
                    });
                    acc
                }),
            transition_id,
        }
    }
}

#[derive(Debug, Clone)]
struct PlaceBindingInfo {
    pub place_id: Uuid,
    pub consumed: HashBag<OCToken>,
}
struct Binding {
    /// Tokens to take out of the place
    pub tokens: HashMap<Uuid, PlaceBindingInfo>,
    pub transition_id: Uuid,
    /// Var arc counts for each object type
    pub var_arc_token_sets: HashMap<String, HashBag<OCToken>>,
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
