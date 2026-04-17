use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, Write};

use sylvan_sys::{BDD, MTBDD, SYLVAN_FALSE, SYLVAN_INVALID, SYLVAN_TRUE};
use sylvan_sys::{
    bdd::{
        Sylvan_and, Sylvan_and_exists, Sylvan_compose, Sylvan_equiv, Sylvan_exists, Sylvan_not,
        Sylvan_or, Sylvan_xor,
    },
    lace::{Task, WorkerP},
    mtbdd::{
        MTBDD_APPLY_OP, Sylvan_high, Sylvan_ithvar, Sylvan_low, Sylvan_map_add, Sylvan_map_empty,
        Sylvan_mtbdd_abstract_max, Sylvan_mtbdd_abstract_min, Sylvan_mtbdd_abstract_plus,
        Sylvan_mtbdd_and_abstract_plus, Sylvan_mtbdd_comp, Sylvan_mtbdd_compose,
        Sylvan_mtbdd_double, Sylvan_mtbdd_equal_norm_d, Sylvan_mtbdd_getdouble,
        Sylvan_mtbdd_hascomp, Sylvan_mtbdd_isleaf, Sylvan_mtbdd_ite, Sylvan_mtbdd_ithvar,
        Sylvan_mtbdd_minus, Sylvan_mtbdd_nodecount, Sylvan_mtbdd_plus, Sylvan_mtbdd_satcount,
        Sylvan_mtbdd_set_from_array, Sylvan_mtbdd_strict_threshold_double, Sylvan_mtbdd_times,
        Sylvan_set_empty, Sylvan_var,
    },
};

use crate::ref_manager::protected_local::{
    ProtectedAddLocal, ProtectedBddLocal, ProtectedMapLocal, ProtectedVarSetLocal,
};
use crate::ref_manager::{AddNode, AddStats, BDDVAR, BddMap, BddNode, EPS, RefManager, VarSet};

#[inline]
fn must_node(n: MTBDD, op: &str) -> MTBDD {
    assert!(n != SYLVAN_INVALID, "Sylvan returned INVALID in {op}");
    n
}

#[inline]
fn is_complemented_raw(node: MTBDD) -> bool {
    unsafe { Sylvan_mtbdd_hascomp(node) != 0 }
}

#[inline]
fn regular_raw(node: MTBDD) -> MTBDD {
    if is_complemented_raw(node) {
        unsafe { Sylvan_mtbdd_comp(node) }
    } else {
        node
    }
}

#[inline]
fn regular_node(node: MTBDD) -> MTBDD {
    regular_raw(node)
}

#[inline]
fn one_bdd() -> BddNode {
    BddNode(SYLVAN_TRUE)
}

#[inline]
fn zero_bdd() -> BddNode {
    BddNode(SYLVAN_FALSE)
}

#[inline]
fn leaf_to_f64(node: MTBDD) -> f64 {
    if node == SYLVAN_FALSE {
        0.0
    } else if node == SYLVAN_TRUE {
        1.0
    } else {
        unsafe { Sylvan_mtbdd_getdouble(node) }
    }
}

extern "C" fn mtbdd_divide_op(
    _w: *mut WorkerP,
    _t: *mut Task,
    a: *mut MTBDD,
    b: *mut MTBDD,
) -> MTBDD {
    unsafe {
        let lhs = *a;
        let rhs = *b;
        if Sylvan_mtbdd_isleaf(lhs) != 0 && Sylvan_mtbdd_isleaf(rhs) != 0 {
            let lv = leaf_to_f64(lhs);
            let rv = leaf_to_f64(rhs);
            return Sylvan_mtbdd_double(lv / rv);
        }
        SYLVAN_INVALID
    }
}

#[inline]
fn var_label(var_index: BDDVAR, labels: &HashMap<BDDVAR, String>) -> String {
    labels
        .get(&var_index)
        .cloned()
        .unwrap_or_else(|| format!("x{}", var_index))
}

#[inline]
fn intern_id(ids: &mut HashMap<MTBDD, usize>, next_id: &mut usize, n: MTBDD) -> usize {
    *ids.entry(n).or_insert_with(|| {
        let id = *next_id;
        *next_id += 1;
        id
    })
}

pub fn var_set_from_indices(_mgr: &RefManager, vars: &[BDDVAR]) -> VarSet {
    let mut arr = vars.to_vec();
    let set = must_node(
        unsafe { Sylvan_mtbdd_set_from_array(arr.as_mut_ptr(), arr.len()) },
        "Sylvan_mtbdd_set_from_array",
    );
    VarSet(set)
}

pub fn var_set_empty(_mgr: &RefManager) -> VarSet {
    VarSet(must_node(unsafe { Sylvan_set_empty() }, "Sylvan_set_empty"))
}

pub fn build_swap_map(mgr: &RefManager, x: &[BDDVAR], y: &[BDDVAR]) -> BddMap {
    let mut map = ProtectedMapLocal::new(BddMap(must_node(
        unsafe { Sylvan_map_empty() },
        "Sylvan_map_empty",
    )));

    for (&xi, &yi) in x.iter().zip(y.iter()) {
        assert!(xi < mgr.next_var_index);
        assert!(yi < mgr.next_var_index);

        let y_var = ProtectedBddLocal::new(BddNode(must_node(
            unsafe { Sylvan_ithvar(yi) },
            "Sylvan_ithvar(y)",
        )));
        let new_map_xy = must_node(
            unsafe { Sylvan_map_add(map.get().0, xi, y_var.get().0) },
            "Sylvan_map_add(x->y)",
        );
        map.set(BddMap(new_map_xy));

        let x_var = ProtectedBddLocal::new(BddNode(must_node(
            unsafe { Sylvan_ithvar(xi) },
            "Sylvan_ithvar(x)",
        )));
        let new_map_yx = must_node(
            unsafe { Sylvan_map_add(map.get().0, yi, x_var.get().0) },
            "Sylvan_map_add(y->x)",
        );
        map.set(BddMap(new_map_yx));
    }

    map.get()
}

pub fn read_var_index(mgr: &RefManager, node: MTBDD) -> BDDVAR {
    if is_constant(mgr, node) {
        BDDVAR::MAX
    } else {
        unsafe { Sylvan_var(regular_node(node)) }
    }
}

pub fn read_then(mgr: &RefManager, node: MTBDD) -> MTBDD {
    if is_constant(mgr, node) {
        regular_node(node)
    } else {
        must_node(unsafe { Sylvan_high(node) }, "Sylvan_high")
    }
}

pub fn read_else(mgr: &RefManager, node: MTBDD) -> MTBDD {
    if is_constant(mgr, node) {
        regular_node(node)
    } else {
        must_node(unsafe { Sylvan_low(node) }, "Sylvan_low")
    }
}

pub fn is_constant(_mgr: &RefManager, node: MTBDD) -> bool {
    unsafe { Sylvan_mtbdd_isleaf(regular_node(node)) != 0 }
}

pub fn add_value(mgr: &RefManager, node: MTBDD) -> Option<f64> {
    if !is_constant(mgr, node) {
        return None;
    }
    if node == SYLVAN_FALSE {
        return Some(0.0);
    }
    if node == SYLVAN_TRUE {
        return Some(1.0);
    }

    let v = leaf_to_f64(regular_node(node));
    if is_complemented_raw(node) {
        Some(1.0 - v)
    } else {
        Some(v)
    }
}

pub fn add_eval_value(mgr: &RefManager, f: AddNode, inputs: &[i32]) -> f64 {
    let required = mgr.var_count();
    assert!(
        inputs.len() >= required,
        "inputs length {} smaller than DD var count {}",
        inputs.len(),
        required
    );

    let mut node = f.0;
    loop {
        if is_constant(mgr, node) {
            return add_value(mgr, node).expect("evaluation must end in constant terminal");
        }
        let var_index = read_var_index(mgr, node) as usize;
        node = if inputs[var_index] == 0 {
            read_else(mgr, node)
        } else {
            read_then(mgr, node)
        };
    }
}

pub fn extract_leftmost_path_from_bdd(mgr: &RefManager, root: BddNode) -> Option<Vec<i32>> {
    let mut inputs = vec![0_i32; mgr.var_count()];
    let zero = zero_bdd().0;
    let mut node = root.0;

    loop {
        if is_constant(mgr, node) {
            return if node == zero { None } else { Some(inputs) };
        }

        let var_index = read_var_index(mgr, node) as usize;
        let else_node = read_else(mgr, node);
        if else_node != zero {
            inputs[var_index] = 0;
            node = else_node;
            continue;
        }

        let then_node = read_then(mgr, node);
        inputs[var_index] = 1;
        node = then_node;
    }
}

pub fn bdd_one(_mgr: &mut RefManager) -> BddNode {
    one_bdd()
}

pub fn bdd_zero(_mgr: &mut RefManager) -> BddNode {
    zero_bdd()
}

pub fn add_zero(mgr: &RefManager) -> AddNode {
    add_const(mgr, 0.0)
}

pub fn add_const(_mgr: &RefManager, value: f64) -> AddNode {
    AddNode(must_node(
        unsafe { Sylvan_mtbdd_double(value) },
        "Sylvan_mtbdd_double",
    ))
}

pub fn bdd_var(mgr: &RefManager, var_index: BDDVAR) -> BddNode {
    assert!(var_index < mgr.next_var_index);
    BddNode(must_node(
        unsafe { Sylvan_ithvar(var_index) },
        "Sylvan_ithvar",
    ))
}

pub fn add_var(mgr: &RefManager, var_index: BDDVAR) -> AddNode {
    assert!(var_index < mgr.next_var_index);
    AddNode(must_node(
        unsafe { Sylvan_mtbdd_ithvar(var_index) },
        "Sylvan_mtbdd_ithvar",
    ))
}

pub fn bdd_not(_mgr: &RefManager, a: BddNode) -> BddNode {
    BddNode(must_node(unsafe { Sylvan_not(a.0) }, "Sylvan_not"))
}

pub fn bdd_equals(_mgr: &RefManager, a: BddNode, b: BddNode) -> BddNode {
    BddNode(must_node(unsafe { Sylvan_equiv(a.0, b.0) }, "Sylvan_equiv"))
}

pub fn bdd_nequals(_mgr: &RefManager, a: BddNode, b: BddNode) -> BddNode {
    BddNode(must_node(unsafe { Sylvan_xor(a.0, b.0) }, "Sylvan_xor"))
}

pub fn bdd_and(_mgr: &RefManager, a: BddNode, b: BddNode) -> BddNode {
    BddNode(must_node(unsafe { Sylvan_and(a.0, b.0) }, "Sylvan_and"))
}

pub fn bdd_or(_mgr: &RefManager, a: BddNode, b: BddNode) -> BddNode {
    BddNode(must_node(unsafe { Sylvan_or(a.0, b.0) }, "Sylvan_or"))
}

pub fn bdd_exists_abstract(_mgr: &RefManager, a: BddNode, vars: VarSet) -> BddNode {
    BddNode(must_node(
        unsafe { Sylvan_exists(a.0, vars.0) },
        "Sylvan_exists",
    ))
}

pub fn bdd_and_then_existsabs(_mgr: &RefManager, f: BddNode, g: BddNode, vars: VarSet) -> BddNode {
    BddNode(must_node(
        unsafe { Sylvan_and_exists(f.0, g.0, vars.0) },
        "Sylvan_and_exists",
    ))
}

pub fn bdd_swap_variables(mgr: &mut RefManager, f: BddNode, x: &[BDDVAR], y: &[BDDVAR]) -> BddNode {
    assert_eq!(x.len(), y.len());
    let map = ProtectedMapLocal::new(build_swap_map(mgr, x, y));
    BddNode(must_node(
        unsafe { Sylvan_compose(f.0, map.get().0) },
        "Sylvan_compose",
    ))
}

pub fn add_swap_vars(mgr: &mut RefManager, f: AddNode, x: &[BDDVAR], y: &[BDDVAR]) -> AddNode {
    assert_eq!(x.len(), y.len());
    let map = ProtectedMapLocal::new(build_swap_map(mgr, x, y));
    AddNode(must_node(
        unsafe { Sylvan_mtbdd_compose(f.0, map.get().0) },
        "Sylvan_mtbdd_compose",
    ))
}

pub fn add_matrix_multiply(mgr: &RefManager, a: AddNode, b: AddNode, z: &[BDDVAR]) -> AddNode {
    let vars = ProtectedVarSetLocal::new(var_set_from_indices(mgr, z));
    add_matrix_multiply_with_var_set(mgr, a, b, vars.get())
}

pub fn add_matrix_multiply_with_var_set(
    _mgr: &RefManager,
    a: AddNode,
    b: AddNode,
    vars: VarSet,
) -> AddNode {
    AddNode(must_node(
        unsafe { Sylvan_mtbdd_and_abstract_plus(a.0, b.0, vars.0) },
        "Sylvan_mtbdd_and_abstract_plus",
    ))
}

pub fn get_var_set_for_indices(mgr: &RefManager, vars: &[BDDVAR]) -> VarSet {
    var_set_from_indices(mgr, vars)
}

pub fn get_swap_map_for_indices(mgr: &mut RefManager, x: &[BDDVAR], y: &[BDDVAR]) -> BddMap {
    build_swap_map(mgr, x, y)
}

pub fn bdd_compose_with_map(_mgr: &mut RefManager, f: BddNode, map: BddMap) -> BddNode {
    BddNode(must_node(
        unsafe { Sylvan_compose(f.0, map.0) },
        "Sylvan_compose",
    ))
}

pub fn add_compose_with_map(_mgr: &mut RefManager, f: AddNode, map: BddMap) -> AddNode {
    AddNode(must_node(
        unsafe { Sylvan_mtbdd_compose(f.0, map.0) },
        "Sylvan_mtbdd_compose",
    ))
}

pub fn add_plus(_mgr: &mut RefManager, a: AddNode, b: AddNode) -> AddNode {
    AddNode(must_node(
        unsafe { Sylvan_mtbdd_plus(a.0, b.0) },
        "Sylvan_mtbdd_plus",
    ))
}

pub fn add_minus(_mgr: &mut RefManager, a: AddNode, b: AddNode) -> AddNode {
    AddNode(must_node(
        unsafe { Sylvan_mtbdd_minus(a.0, b.0) },
        "Sylvan_mtbdd_minus",
    ))
}

pub fn add_times(_mgr: &mut RefManager, a: AddNode, b: AddNode) -> AddNode {
    AddNode(must_node(
        unsafe { Sylvan_mtbdd_times(a.0, b.0) },
        "Sylvan_mtbdd_times",
    ))
}

pub fn add_divide(_mgr: &mut RefManager, a: AddNode, b: AddNode) -> AddNode {
    let op: MTBDD_APPLY_OP = mtbdd_divide_op;
    AddNode(must_node(
        unsafe { sylvan_sys::mtbdd::Sylvan_mtbdd_apply(a.0, b.0, op) },
        "Sylvan_mtbdd_apply(divide)",
    ))
}

pub fn add_ite(
    _mgr: &mut RefManager,
    cond: BddNode,
    then_branch: AddNode,
    else_branch: AddNode,
) -> AddNode {
    AddNode(must_node(
        unsafe { Sylvan_mtbdd_ite(cond.0, then_branch.0, else_branch.0) },
        "Sylvan_mtbdd_ite",
    ))
}

pub fn add_sum_abstract(_mgr: &RefManager, f: AddNode, vars: VarSet) -> AddNode {
    AddNode(must_node(
        unsafe { Sylvan_mtbdd_abstract_plus(f.0, vars.0) },
        "Sylvan_mtbdd_abstract_plus",
    ))
}

pub fn add_or_abstract(mgr: &RefManager, f: AddNode, vars: VarSet) -> AddNode {
    add_max_abstract(mgr, f, vars)
}

pub fn add_max_abstract(_mgr: &RefManager, f: AddNode, vars: VarSet) -> AddNode {
    AddNode(must_node(
        unsafe { Sylvan_mtbdd_abstract_max(f.0, vars.0) },
        "Sylvan_mtbdd_abstract_max",
    ))
}

pub fn add_min_abstract(_mgr: &RefManager, f: AddNode, vars: VarSet) -> AddNode {
    AddNode(must_node(
        unsafe { Sylvan_mtbdd_abstract_min(f.0, vars.0) },
        "Sylvan_mtbdd_abstract_min",
    ))
}

pub fn add_to_bdd(_mgr: &mut RefManager, a: AddNode) -> BddNode {
    BddNode(must_node(
        unsafe { Sylvan_mtbdd_strict_threshold_double(a.0, EPS) },
        "Sylvan_mtbdd_strict_threshold_double",
    ))
}

pub fn add_to_bdd_pattern(mgr: &mut RefManager, a: AddNode) -> BddNode {
    let zero_for_gt = ProtectedAddLocal::new(add_const(mgr, 0.0));
    let gt_zero = ProtectedBddLocal::new(add_greater_than(mgr, a, zero_for_gt.get()));
    let zero_for_lt = ProtectedAddLocal::new(add_const(mgr, 0.0));
    let lt_zero = ProtectedBddLocal::new(add_less_than(mgr, a, zero_for_lt.get()));
    bdd_or(mgr, gt_zero.get(), lt_zero.get())
}

pub fn bdd_to_add(mgr: &mut RefManager, b: BddNode) -> AddNode {
    let one = ProtectedAddLocal::new(add_const(mgr, 1.0));
    let zero = ProtectedAddLocal::new(add_const(mgr, 0.0));
    AddNode(must_node(
        unsafe { Sylvan_mtbdd_ite(b.0, one.get().0, zero.get().0) },
        "Sylvan_mtbdd_ite(bdd_to_add)",
    ))
}

pub fn add_greater_than(mgr: &mut RefManager, a: AddNode, b: AddNode) -> BddNode {
    let diff = ProtectedAddLocal::new(add_minus(mgr, a, b));
    add_to_bdd(mgr, diff.get())
}

pub fn add_less_than(mgr: &mut RefManager, a: AddNode, b: AddNode) -> BddNode {
    let diff = ProtectedAddLocal::new(add_minus(mgr, b, a));
    add_to_bdd(mgr, diff.get())
}

pub fn add_greater_or_equal(mgr: &mut RefManager, a: AddNode, b: AddNode) -> BddNode {
    let lt = ProtectedBddLocal::new(add_less_than(mgr, a, b));
    bdd_not(mgr, lt.get())
}

pub fn add_less_or_equal(mgr: &mut RefManager, a: AddNode, b: AddNode) -> BddNode {
    let gt = ProtectedBddLocal::new(add_greater_than(mgr, a, b));
    bdd_not(mgr, gt.get())
}

pub fn add_equals(mgr: &mut RefManager, a: AddNode, b: AddNode) -> BddNode {
    let gt = ProtectedBddLocal::new(add_greater_than(mgr, a, b));
    let lt = ProtectedBddLocal::new(add_less_than(mgr, a, b));
    let neq = ProtectedBddLocal::new(bdd_or(mgr, gt.get(), lt.get()));
    bdd_not(mgr, neq.get())
}

pub fn add_nequals(mgr: &mut RefManager, a: AddNode, b: AddNode) -> BddNode {
    let gt = ProtectedBddLocal::new(add_greater_than(mgr, a, b));
    let lt = ProtectedBddLocal::new(add_less_than(mgr, a, b));
    bdd_or(mgr, gt.get(), lt.get())
}

pub fn add_equal_sup_norm(_mgr: &RefManager, a: AddNode, b: AddNode, tolerance: f64) -> bool {
    unsafe { Sylvan_mtbdd_equal_norm_d(a.0, b.0, tolerance) == SYLVAN_TRUE }
}

pub fn epsilon(_mgr: &RefManager) -> f64 {
    EPS
}

pub fn bdd_count_minterms(_mgr: &RefManager, rel: BddNode, num_vars: u32) -> u64 {
    unsafe { Sylvan_mtbdd_satcount(rel.0, num_vars as usize) }.round() as u64
}

pub fn dag_size(_mgr: &RefManager, root: MTBDD) -> usize {
    unsafe { Sylvan_mtbdd_nodecount(regular_node(root)) as usize }
}

pub fn foreach_node<F: FnMut(MTBDD)>(mgr: &RefManager, root: MTBDD, mut f: F) {
    let mut visited: HashSet<MTBDD> = HashSet::new();
    let mut stack = vec![regular_node(root)];

    while let Some(node) = stack.pop() {
        let node = regular_node(node);
        if !visited.insert(node) {
            continue;
        }
        f(node);
        if !is_constant(mgr, node) {
            stack.push(read_then(mgr, node));
            stack.push(read_else(mgr, node));
        }
    }
}

pub fn terminal_nodes(mgr: &RefManager, root: MTBDD) -> Vec<MTBDD> {
    let mut out = Vec::new();
    foreach_node(mgr, root, |n| {
        if is_constant(mgr, n) {
            out.push(regular_node(n));
        }
    });
    out.sort_unstable();
    out.dedup();
    out
}

pub fn num_terminals(mgr: &RefManager, root: MTBDD) -> usize {
    terminal_nodes(mgr, root).len()
}

pub fn num_nodes(mgr: &RefManager, node: MTBDD) -> usize {
    dag_size(mgr, node)
}

pub fn add_stats(mgr: &RefManager, root: AddNode, num_vars: u32) -> AddStats {
    let root = regular_node(root.0);
    let minterms =
        unsafe { sylvan_sys::mtbdd::Sylvan_mtbdd_satcount(root, num_vars as usize) }.round() as u64;
    AddStats {
        node_count: dag_size(mgr, root),
        terminal_count: num_terminals(mgr, root),
        minterms,
    }
}

fn var_index_label_map(
    mgr: &RefManager,
    var_names: &HashMap<BDD, String>,
) -> HashMap<BDDVAR, String> {
    let mut labels = HashMap::new();
    for (&node, name) in var_names {
        let var_index = read_var_index(mgr, node);
        if var_index != BDDVAR::MAX {
            labels.entry(var_index).or_insert_with(|| name.clone());
        }
    }
    labels
}

fn dump_add_dot_rec<W: Write>(
    mgr: &RefManager,
    n: MTBDD,
    out: &mut W,
    labels: &HashMap<BDDVAR, String>,
    ids: &mut HashMap<MTBDD, usize>,
    next_id: &mut usize,
    visited: &mut HashSet<MTBDD>,
) -> io::Result<()> {
    let n = regular_node(n);
    if !visited.insert(n) {
        return Ok(());
    }

    let this = intern_id(ids, next_id, n);
    let var = read_var_index(mgr, n);
    if var == BDDVAR::MAX {
        let v = add_value(mgr, n).unwrap_or(f64::NAN);
        writeln!(out, "  n{} [shape=box,label=\"{}\"] ;", this, v)?;
        return Ok(());
    }

    let t = regular_node(read_then(mgr, n));
    let e = regular_node(read_else(mgr, n));
    let tid = intern_id(ids, next_id, t);
    let eid = intern_id(ids, next_id, e);
    let label = var_label(var, labels);

    writeln!(out, "  n{} [shape=ellipse,label=\"{}\"] ;", this, label)?;
    writeln!(out, "  n{} -> n{};", this, tid)?;
    writeln!(out, "  n{} -> n{} [style=dashed];", this, eid)?;

    dump_add_dot_rec(mgr, t, out, labels, ids, next_id, visited)?;
    dump_add_dot_rec(mgr, e, out, labels, ids, next_id, visited)?;
    Ok(())
}

pub fn dump_add_dot(
    mgr: &RefManager,
    root: AddNode,
    path: &str,
    var_names: &HashMap<BDD, String>,
) -> io::Result<()> {
    let mut out = File::create(path)?;
    writeln!(out, "digraph ADD {{")?;
    writeln!(out, "  rankdir=TB;")?;

    let mut ids: HashMap<MTBDD, usize> = HashMap::new();
    let mut next_id = 0usize;
    let mut visited: HashSet<MTBDD> = HashSet::new();
    let labels = var_index_label_map(mgr, var_names);

    let root_reg = regular_node(root.0);
    dump_add_dot_rec(
        mgr,
        root_reg,
        &mut out,
        &labels,
        &mut ids,
        &mut next_id,
        &mut visited,
    )?;
    writeln!(out, "}}")?;
    Ok(())
}

pub fn dump_bdd_dot(
    mgr: &RefManager,
    root: BddNode,
    path: &str,
    var_names: &HashMap<BDD, String>,
) -> io::Result<()> {
    dump_add_dot(mgr, AddNode(root.0), path, var_names)
}

pub fn get_encoding(mgr: &mut RefManager, indices: &[BDDVAR]) -> AddNode {
    let mut result = ProtectedAddLocal::new(add_const(mgr, 0.0));
    let bdd_one_node = ProtectedBddLocal::new(bdd_one(mgr));

    for bm in 0..(1i32 << indices.len()) {
        let mut term = ProtectedBddLocal::new(bdd_one_node.get());
        for (i, &var) in indices.iter().enumerate() {
            let literal = ProtectedBddLocal::new(if (bm & (1 << i)) != 0 {
                bdd_var(mgr, var)
            } else {
                let var_node = ProtectedBddLocal::new(bdd_var(mgr, var));
                bdd_not(mgr, var_node.get())
            });
            term.set(bdd_and(mgr, term.get(), literal.get()));
        }
        let term_as_add = ProtectedAddLocal::new(bdd_to_add(mgr, term.get()));
        let value = ProtectedAddLocal::new(add_const(mgr, bm as f64));
        let weighted_term = ProtectedAddLocal::new(add_times(mgr, term_as_add.get(), value.get()));
        result.set(add_plus(mgr, result.get(), weighted_term.get()));
    }

    result.get()
}

pub fn unif(mgr: &mut RefManager, m: AddNode, vars: VarSet) -> AddNode {
    let denom = ProtectedAddLocal::new(add_sum_abstract(mgr, m, vars));
    let denom_bdd = ProtectedBddLocal::new(add_to_bdd(mgr, denom.get()));
    let one = ProtectedAddLocal::new(add_const(mgr, 1.0));
    let denom_safe = ProtectedAddLocal::new(add_ite(mgr, denom_bdd.get(), denom.get(), one.get()));
    add_divide(mgr, m, denom_safe.get())
}
