use lazy_static::lazy_static;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

/// A global state to keep track of the current permutation indices for each base list.
lazy_static! {
    static ref PERMUTATION_STATE: Mutex<HashMap<u64, Vec<usize>>> = Mutex::new(HashMap::new());
}

/// Computes a unique hash for the base list.
///
/// **Note:** This method requires that the elements of the base list implement the `Hash` trait.
/// Collisions are possible but highly unlikely with a good hash function.
fn hash_base_list<T: Hash>(base_list: &Vec<T>) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    base_list.hash(&mut hasher);
    hasher.finish()
}

/// Generates the next lexicographical permutation for a vector of indices.
///
/// Returns `true` if the next permutation was successfully generated,
/// or `false` if it has wrapped around to the first permutation.
fn generate_next_permutation(indices: &mut Vec<usize>) -> bool {
    // Step 1: Find the largest index `i` such that indices[i] < indices[i + 1]
    let mut i = indices.len().saturating_sub(2);
    while i < indices.len() {
        if indices[i] < indices[i + 1] {
            break;
        }
        if i == 0 {
            return false; // Last permutation reached
        }
        i = i.saturating_sub(1);
    }

    // Step 2: Find the largest index `j` greater than `i` such that indices[j] > indices[i]
    let mut j = indices.len() - 1;
    while indices[j] <= indices[i] {
        j = j.saturating_sub(1);
    }

    // Step 3: Swap indices[i] and indices[j]
    indices.swap(i, j);

    // Step 4: Reverse the sequence from indices[i + 1] up to the end
    indices[i + 1..].reverse();

    true
}

/// Generates the next deterministic permutation of the provided base list.
///
/// # Arguments
///
/// * `base_list` - A reference to the base list to permute.
///
/// # Returns
///
/// A new `Vec<T>` representing the next permutation in lexicographical order.
/// If the last permutation was reached, it wraps around to the first permutation.
///
/// # Constraints
///
/// - `T` must implement `Hash` and `Clone`.
///
/// # Example
///
/// ```rust
/// let base = vec![1, 2, 3];
/// let perm1 = next_permutation(&base); // [1, 2, 3]
/// let perm2 = next_permutation(&base); // [1, 3, 2]
/// ```
pub fn next_permutation<T>(base_list: &Vec<T>) -> Vec<T>
where
    T: Hash + Clone,
{
    // Compute a unique key for the base list
    let key = hash_base_list(base_list);

    // Acquire the lock for the global permutation state
    let mut state = PERMUTATION_STATE.lock().unwrap();

    // Retrieve the current permutation indices or initialize them to the first permutation
    let current_permutation = state
        .get(&key)
        .cloned()
        .unwrap_or_else(|| (0..base_list.len()).collect());

    // Clone the current permutation indices to generate the next permutation
    let mut next_perm_indices = current_permutation.clone();

    // Attempt to generate the next permutation
    let has_next = generate_next_permutation(&mut next_perm_indices);

    if !has_next {
        // If it's the last permutation, reset to the first permutation
        next_perm_indices = (0..base_list.len()).collect();
    }

    // Update the global state with the new permutation indices
    state.insert(key, next_perm_indices.clone());

    // Apply the permutation indices to the base list to generate the next permutation
    next_perm_indices
        .iter()
        .map(|&i| base_list[i].clone())
        .collect()
}
