use std::collections::BTreeMap;

use graphviz_rust::dot_structures::{Attribute, EdgeTy, Graph, Id, Stmt, Vertex};
use petgraph::graph::NodeIndex;
use std::collections::HashMap;
use gcl::parse;

#[derive(Debug)]
pub struct ParsedGraph {
    pub nodes: BTreeMap<String, Node>,
    pub node_mapping: BTreeMap<String, NodeIndex>,
    pub graph: petgraph::Graph<String, gcl::pg::Action>,
}

#[derive(Debug, Default)]
pub struct Node {
    pub attributes: Vec<Attribute>,
    pub outgoing: Vec<String>,
    pub ingoing: Vec<String>,
}

//Generates an edge list from the provided DOT representation
fn dot_to_edge_list(dot: &str) -> Vec<(String, String, String)> {
    let mut edge_list = Vec::new();
    
    let parsed = graphviz_rust::parse(dot).unwrap();
    match parsed {
        Graph::Graph { .. } => todo!(),
        Graph::DiGraph { stmts, .. } => {
            for stmt in stmts {
                match stmt {
                    Stmt::Edge(e) => {
                        match e.ty {
                            EdgeTy::Pair(graphviz_rust::dot_structures::Vertex::N(a), graphviz_rust::dot_structures::Vertex::N(b)) => {
                                let label = e
                                    .attributes
                                    .iter()
                                    .find_map(|a| match (&a.0, &a.1) {
                                        (Id::Plain(l), Id::Escaped(v)) if l == "label" => {
                                            Some(v.to_string())
                                        }
                                        _ => None,
                                    })
                                    .ok_or("edge label not found").unwrap();
                                let label = label.trim_matches('"').to_owned();
                                match (a.0, b.0) {
                                    (graphviz_rust::dot_structures::Id::Plain(a), graphviz_rust::dot_structures::Id::Plain(b)) => edge_list.push((a, label, b)),
                                    _ => {}
                                }
                            },
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    
    edge_list
}

//Counts the number of nodes in an edge list
fn count_nodes(edge_list : &Vec<(String, String, String)>) -> usize {
    let mut count = 0;
    
    let mut count_map : HashMap<String, i32> = HashMap::new();
    for e in edge_list {
        if !count_map.contains_key(&e.0) {
            count_map.insert(e.0.clone(), 0);
            count += 1;
        }
        
        if !count_map.contains_key(&e.2) {
            count_map.insert(e.2.clone(), 0);
            count += 1;
        }
        
    }
    
    count
}

//Converts an edge list to an adjacency list
fn edge_list_to_adj_list(edge_list: &Vec<(String, String, String)>) -> Vec<Vec<(usize, String)>> {
    let mut adj_list : Vec<Vec<(usize, String)>> = Vec::new();
    
    for _ in 0..count_nodes(&edge_list) {
        adj_list.push(Vec::new());
    }
    
    let mut ref_map : HashMap<String, usize> = HashMap::new();
    let mut id : usize = 0;
    
    for e in edge_list {
        if !ref_map.contains_key(&e.0) {
            ref_map.insert(e.0.clone(), id);
            id += 1;
        }
        
        if !ref_map.contains_key(&e.2) {
            ref_map.insert(e.2.clone(), id);
            id += 1;
        }
        
        adj_list.get_mut(ref_map.get(&e.0).unwrap().clone()).unwrap().push((ref_map.get(&e.2).unwrap().clone(), e.1.clone()));
        
    }
    
    adj_list
}

fn direct_check(expr1 : &str, expr2 : &str) -> bool {
    let cexpr1 = parse::parse_commands(expr1);
    let cexpr2 = parse::parse_commands(expr2);
    let bexpr1 = parse::parse_bexpr(expr1);
    let bexpr2 = parse::parse_bexpr(expr2);
    
    if (cexpr1.is_ok() && cexpr2.is_ok() && cexpr1.unwrap() == cexpr2.unwrap()) || (bexpr1.is_ok() && bexpr2.is_ok() && bexpr1.unwrap() == bexpr2.unwrap()) {
        return true
    }
    return false
}

//Checks whether two dots are equivalent
pub fn simple_check_eq(dot1: &str, dot2: &str) -> bool {
    let mut res = true;
    
    //Step 1: Convert dot format to <NodeId, String, NodeId> format.
    let edge_list_dot1 = dot_to_edge_list(dot1);
    let edge_list_dot2 = dot_to_edge_list(dot2);
    
    //tracing::info!(?edge_list_dot1, "Edge List 1: ");
    //tracing::info!(?edge_list_dot2, "Edge List 2: ");
    
    //Step 2: Create adjacency list for each one
    //let adj_dot1: Vec<Vec<(usize, String)>> = edge_list_to_adj_list(&edge_list_dot1); //Dest, Label
    //let adj_dot2: Vec<Vec<(usize, String)>> = edge_list_to_adj_list(&edge_list_dot2); //Dest, Label
    
    //Step 3: Simple Check, do all labels exist in both systems from Step 1? (0 Layer OK Check)
    if edge_list_dot1.len() != edge_list_dot2.len() {
        res = false;
    } else {
        for e1 in &edge_list_dot1 {
            let mut found = false;
            for e2 in &edge_list_dot2 {
                if direct_check(&e1.1, &e2.1) { //Consider changing this parameter, in regards to checking equivalence
                    found = true;
                    break
                }
            }
            
            if !found {
                res = false;
                break
            }
        }
    }
    
    //Step 4: For each node in adj_dot1, can a node be found in the other, with the same labels? (1 Layer OK Check)
    /*for adj in adj_dot1 {
        
    }*/
    
    res
}

pub fn dot_to_petgraph(dot: &str) -> Result<ParsedGraph, String> {
    let mut nodes = BTreeMap::<String, Node>::new();
    let mut node_mapping = BTreeMap::<String, NodeIndex>::new();
    let mut graph = petgraph::Graph::<String, gcl::pg::Action>::new();

    let parsed = graphviz_rust::parse(dot)?;

    match parsed {
        Graph::Graph { .. } => todo!(),
        Graph::DiGraph { stmts, .. } => {
            for stmt in stmts {
                match stmt {
                    Stmt::Node(n) => {
                        node_mapping
                            .entry(n.id.0.to_string())
                            .or_insert_with_key(|k| graph.add_node(k.to_string()));

                        nodes
                            .entry(n.id.0.to_string())
                            .or_default()
                            .attributes
                            .extend_from_slice(&n.attributes);
                    }
                    Stmt::Subgraph(_) => {}
                    Stmt::Attribute(_) => {}
                    Stmt::GAttribute(_) => {}
                    Stmt::Edge(e) => match e.ty {
                        EdgeTy::Pair(a, b) => {
                            if let (Vertex::N(a), Vertex::N(b)) = (a, b) {
                                let a_id = *node_mapping
                                    .entry(a.0.to_string())
                                    .or_insert_with_key(|k| graph.add_node(k.to_string()));
                                let b_id = *node_mapping
                                    .entry(b.0.to_string())
                                    .or_insert_with_key(|k| graph.add_node(k.to_string()));
                                let label = e
                                    .attributes
                                    .iter()
                                    .find_map(|a| match (&a.0, &a.1) {
                                        (Id::Plain(l), Id::Escaped(v)) if l == "label" => {
                                            Some(v.to_string())
                                        }
                                        _ => None,
                                    })
                                    .ok_or("edge label not found")?;
                                let label = label.trim_matches('"');
                                let action = gcl::parse::parse_action(label)
                                    .map_err(|e| format!("failed to parse action: {label}. {e}"))?;
                                graph.add_edge(a_id, b_id, action);

                                nodes
                                    .entry(a.0.to_string())
                                    .or_default()
                                    .outgoing
                                    .push(b.0.to_string());
                                nodes
                                    .entry(b.0.to_string())
                                    .or_default()
                                    .ingoing
                                    .push(a.0.to_string());
                            }
                        }
                        EdgeTy::Chain(_) => {}
                    },
                }
            }
        }
    }

    Ok(ParsedGraph {
        nodes,
        node_mapping,
        graph,
    })
}
