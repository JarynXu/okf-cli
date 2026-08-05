//! Directed graph views over document references.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use serde::{Deserialize, Serialize};
use crate::model::{Bundle, DocumentId};

/// A deterministic directed graph derived from resolved document references.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct KnowledgeGraph {
    nodes: BTreeSet<DocumentId>,
    outgoing: BTreeMap<DocumentId, BTreeSet<DocumentId>>,
    incoming: BTreeMap<DocumentId, BTreeSet<DocumentId>>,
    relations: BTreeMap<(DocumentId, DocumentId), BTreeSet<String>>,
}
impl KnowledgeGraph {
    /// Builds a graph from references that resolve to documents in `bundle`.
    pub fn from_bundle(bundle: &Bundle) -> Self {
        let mut graph = Self::default();
        for document in bundle.documents() {
            graph.nodes.insert(document.id().clone());
            graph.outgoing.entry(document.id().clone()).or_default();
            graph.incoming.entry(document.id().clone()).or_default();
        }
        for document in bundle.documents() {
            for reference in &document.metadata().links {
                let Some(target) = bundle.resolve(reference.target().as_str()) else { continue; };
                let source_id = document.id().clone();
                let target_id = target.id().clone();
                graph.outgoing.entry(source_id.clone()).or_default().insert(target_id.clone());
                graph.incoming.entry(target_id.clone()).or_default().insert(source_id.clone());
                graph.relations.entry((source_id, target_id)).or_default().insert(reference.relation().to_owned());
            }
        }
        graph
    }
    /// Returns the number of documents represented by the graph.
    pub fn node_count(&self) -> usize { self.nodes.len() }
    /// Returns the number of unique directed source-target pairs.
    pub fn edge_count(&self) -> usize { self.relations.len() }
    /// Returns whether the graph contains a document identifier.
    pub fn contains(&self, id: &DocumentId) -> bool { self.nodes.contains(id) }
    /// Iterates over graph nodes in identifier order.
    pub fn nodes(&self) -> impl ExactSizeIterator<Item = &DocumentId> { self.nodes.iter() }
    /// Iterates over direct outgoing neighbors in identifier order.
    pub fn outgoing<'a>(&'a self, id: &'a DocumentId) -> impl Iterator<Item = &'a DocumentId> + 'a { self.outgoing.get(id).into_iter().flat_map(|neighbors| neighbors.iter()) }
    /// Iterates over direct incoming neighbors in identifier order.
    pub fn incoming<'a>(&'a self, id: &'a DocumentId) -> impl Iterator<Item = &'a DocumentId> + 'a { self.incoming.get(id).into_iter().flat_map(|neighbors| neighbors.iter()) }
    /// Iterates over relation labels for a directed source-target pair.
    pub fn relations<'a>(&'a self, source: &DocumentId, target: &DocumentId) -> impl Iterator<Item = &'a str> + 'a { self.relations.get(&(source.clone(), target.clone())).into_iter().flat_map(|neighbors| neighbors.iter()).map(String::as_str) }
    /// Returns documents with no resolved incoming references.
    pub fn roots(&self) -> Vec<&DocumentId> { self.nodes.iter().filter(|id| self.incoming.get(*id).is_none_or(BTreeSet::is_empty)).collect() }
    /// Returns documents with no resolved outgoing references.
    pub fn leaves(&self) -> Vec<&DocumentId> { self.nodes.iter().filter(|id| self.outgoing.get(*id).is_none_or(BTreeSet::is_empty)).collect() }
    /// Returns the start node and every node reachable from it.
    pub fn reachable_from(&self, start: &DocumentId) -> BTreeSet<DocumentId> {
        if !self.contains(start) { return BTreeSet::new(); }
        let mut visited = BTreeSet::new();
        let mut queue = VecDeque::from([start.clone()]);
        while let Some(current) = queue.pop_front() {
            if !visited.insert(current.clone()) { continue; }
            if let Some(neighbors) = self.outgoing.get(&current) { queue.extend(neighbors.iter().filter(|id| !visited.contains(*id)).cloned()); }
        }
        visited
    }
    /// Finds a deterministic shortest directed path, including both endpoints.
    pub fn shortest_path(&self, start: &DocumentId, goal: &DocumentId) -> Option<Vec<DocumentId>> {
        if !self.contains(start) || !self.contains(goal) { return None; }
        if start == goal { return Some(vec![start.clone()]); }
        let mut parents = BTreeMap::<DocumentId, DocumentId>::new();
        let mut visited = BTreeSet::from([start.clone()]);
        let mut queue = VecDeque::from([start.clone()]);
        while let Some(current) = queue.pop_front() {
            for neighbor in self.outgoing(&current) {
                if !visited.insert(neighbor.clone()) { continue; }
                parents.insert(neighbor.clone(), current.clone());
                if neighbor == goal { return Some(reconstruct_path(start, goal, &parents)); }
                queue.push_back(neighbor.clone());
            }
        }
        None
    }
}
fn reconstruct_path(start: &DocumentId, goal: &DocumentId, parents: &BTreeMap<DocumentId, DocumentId>) -> Vec<DocumentId> {
    let mut path = vec![goal.clone()];
    let mut current = goal;
    while current != start {
        let Some(parent) = parents.get(current) else { return Vec::new(); };
        path.push(parent.clone());
        current = parent;
    }
    path.reverse();
    path
}
