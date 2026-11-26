//! MIR dataflow analysis framework and standard analyses (rustc's `rustc_mir_dataflow`).

pub mod framework;
pub mod impls;
pub mod move_paths;

pub use framework::{
    Analysis, Backward, CallReturnPlaces, Direction, Forward, GenKill, JoinSemiLattice, Location,
    Results, ResultsCursor, ResultsVisitor, iterate_to_fixpoint, predecessors, reverse_postorder,
    visit_results,
};
pub use move_paths::{
    Init, InitIndex, InitKind, InitLocation, LookupResult, MoveData, MoveOut, MoveOutIndex,
    MovePath, MovePathIndex, place_ty, type_is_copy,
};
