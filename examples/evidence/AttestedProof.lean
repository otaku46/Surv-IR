import Evidence
namespace Surv_evidence

private def known : List State :=
  initial :: (successors initial).flatMap fun s => s :: successors s

private theorem known_closed :
    (known.all fun s => (successors s).all (fun t => known.contains t)) = true := by
  decide

private theorem known_safe :
    (known.all fun s => invariant_0_attested_has_bound_verification s == some true) = true := by
  decide

private theorem obligations :
    InvariantObligations (fun s => s ∈ known)
      (fun s => invariant_0_attested_has_bound_verification s = some true) where
  initial := by decide
  preservation := by
    intro before after step before_known
    have successors_known := List.all_eq_true.mp known_closed before before_known
    exact List.contains_iff_mem.mp (List.all_eq_true.mp successors_known after step)
  implication := by
    intro s member
    exact LawfulBEq.eq_of_beq (List.all_eq_true.mp known_safe s member)

theorem attested_has_bound_verification {s : State} (reachable : Reachable s) :
    invariant_0_attested_has_bound_verification s = some true :=
  prove_invariant_0_attested_has_bound_verification obligations reachable

private def selected (index : Nat) : State :=
  (successors initial)[index]?.getD initial

private def evaluated (index : Nat) : State :=
  (successors (selected index))[0]?.getD initial

theorem bound_is_attested :
    ∃ s, Reachable s ∧ goal_0_bound_is_attested s = some true := by
  refine ⟨evaluated 0, Reachable.step (before := selected 0)
    (Reachable.step Reachable.initial (by decide)) (by decide), by decide⟩

theorem unrelated_is_recorded_only :
    ∃ s, Reachable s ∧ goal_1_unrelated_is_recorded_only s = some true := by
  refine ⟨evaluated 1, Reachable.step (before := selected 1)
    (Reachable.step Reachable.initial (by decide)) (by decide), by decide⟩

#print axioms attested_has_bound_verification
#print axioms bound_is_attested
#print axioms unrelated_is_recorded_only

end Surv_evidence
