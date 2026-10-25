# Good and Bad Tests

## Good Tests

**Integration-style at the seam**: Test through real public interfaces, not mocks of internal parts.

```rust
// GOOD: Tests observable state transitions at the public seam
#[test]
fn active_agent_forces_working_state_until_stopped() {
    let mut state = AppStateInner::new(41905);
    state.active_agents = 1;
    let snap = state.reconcile();
    assert_eq!(snap.cat_state, CatState::Working);
    assert!(snap.preventing_sleep);

    state.active_agents = 0;
    let snap = state.reconcile();
    assert_eq!(snap.cat_state, CatState::Sleeping);
    assert!(!snap.preventing_sleep);
}
```

Characteristics:

- Tests behavior users/callers care about
- Uses public API only
- Survives internal refactors
- Describes WHAT, not HOW
- One logical scenario per test

## Bad Tests

**Implementation-detail tests**: Coupled to internal structure.

```typescript
// BAD: Tests implementation details
test("checkout calls paymentService.process", async () => {
  const mockPayment = jest.mock(paymentService);
  await checkout(cart, payment);
  expect(mockPayment.process).toHaveBeenCalledWith(cart.total);
});
```

Red flags:

- Mocking internal collaborators
- Testing private functions/fields directly instead of observing `AppStateSnapshot` or return values
- Asserting on call counts/order of internal helpers
- Test breaks when refactoring without behavior change
- Test name describes HOW not WHAT

**Tautological tests**: Expected value restates the implementation, so the test passes by construction.

```rust
// BAD: Expected value is recomputed the same way the code computes it
#[test]
fn policy_30m_duration() {
    assert_eq!(
        StayAwakePolicy::Minutes30.duration(),
        Some(Duration::from_secs(30 * 60))
    );
}

// GOOD: Expected value is an independent, known literal
#[test]
fn policy_30m_is_1800_seconds() {
    assert_eq!(
        StayAwakePolicy::Minutes30.duration(),
        Some(Duration::from_secs(1800))
    );
}
```
