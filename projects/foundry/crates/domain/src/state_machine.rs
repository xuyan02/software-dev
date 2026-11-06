/// A type-level state machine.
///
/// Status enums implement this trait so the application layer can validate
/// transitions without knowing the concrete workflow.
pub trait StateMachine: Copy + Eq {
    fn can_transition_to(self, next: Self) -> bool;
}
