use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use std::sync::{RwLock};
use lazy_static::lazy_static;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct ObjectType(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct EventType(pub usize);

impl fmt::Display for ObjectType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ts = TYPE_STORAGE.read().unwrap();
        let name = ts.get_type_name(self.0).unwrap_or("Unknown");
        write!(f, "{}", name)
    }
}

impl fmt::Display for EventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ts = TYPE_STORAGE.read().unwrap();
        let name = ts.get_type_name(self.0).unwrap_or("Unknown");
        write!(f, "{}", name)
    }
}

impl From<ObjectType> for usize {
    #[inline(always)]
    fn from(obj: ObjectType) -> usize {
        obj.0
    }
}

impl From<usize> for ObjectType {
    #[inline(always)]
    fn from(id: usize) -> ObjectType {
        ObjectType(id)
    }
}

impl From<EventType> for usize {
    #[inline(always)]
    fn from(event: EventType) -> usize {
        event.0
    }
}

impl From<usize> for EventType {
    #[inline(always)]
    fn from(id: usize) -> EventType {
        EventType(id)
    }
}

impl From<String> for EventType {
    fn from(type_name: String) -> Self {
        let mut ts = TYPE_STORAGE.write().unwrap();
        EventType(ts.get_or_insert_type_id(&type_name))
    }
}

impl From<String> for ObjectType {
    fn from(type_name: String) -> Self {
        let mut ts = TYPE_STORAGE.write().unwrap();
        ObjectType(ts.get_or_insert_type_id(&type_name))
    }
}

impl From<&str> for EventType {
    fn from(type_name: &str) -> Self {
        let mut ts = TYPE_STORAGE.write().unwrap();
        EventType(ts.get_or_insert_type_id(type_name))
    }
}

impl From<&str> for ObjectType {
    fn from(type_name: &str) -> Self {
        let mut ts = TYPE_STORAGE.write().unwrap();
        ObjectType(ts.get_or_insert_type_id(type_name))
    }
}

pub struct TypeStorage {
    types: HashMap<String, usize>,
    ids: Vec<String>,
}

impl TypeStorage {
    pub fn new() -> Self {
        TypeStorage {
            types: HashMap::new(),
            ids: Vec::new(),
        }
    }

    pub fn get_type_id(&self, type_name: &str) -> Option<usize> {
        self.types.get(type_name).copied()
    }
    
    pub fn get_or_insert_type_id(&mut self, type_name:  &str) -> usize {
        if let Some(&type_id) = self.types.get(type_name) {
            type_id
        } else {
            let type_id = self.ids.len();
            self.types.insert(type_name.to_string(), type_id);
            self.ids.push(type_name.to_string());
            type_id
        }
    }

    pub fn get_type_name(&self, type_id: usize) -> Option<&str> {
        self.ids.get(type_id).map(|s| s.as_str())
    }
}

lazy_static! {
    pub static ref TYPE_STORAGE: RwLock<TypeStorage> = RwLock::new(TypeStorage::new());
}