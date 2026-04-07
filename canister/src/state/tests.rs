mod assert_caller_is_allowed {
    use crate::state::State;
    use crate::test_fixtures::mocks::MockRuntime;
    use candid::Principal;
    use dex_types_internal::Mode;

    #[test]
    fn should_allow_any_caller_in_general_availability_mode() {
        let state = state(Mode::GeneralAvailability);
        let mock = MockRuntime::new();
        state.assert_caller_is_allowed(&mock);
    }

    #[test]
    fn should_allow_caller_in_restricted_set() {
        let allowed_principal = Principal::from_slice(&[0x01]);
        let state = state(Mode::restricted_to(vec![allowed_principal]));

        let mut mock = MockRuntime::new();
        mock.expect_msg_caller().return_const(allowed_principal);
        mock.expect_is_controller().return_const(false);

        state.assert_caller_is_allowed(&mock);
    }

    #[test]
    fn should_allow_controller_even_if_not_in_restricted_set() {
        let state = state(Mode::restricted_to(vec![]));
        let controller = Principal::from_slice(&[0xAA]);

        let mut mock = MockRuntime::new();
        mock.expect_msg_caller().return_const(controller);
        mock.expect_is_controller().return_const(true);

        state.assert_caller_is_allowed(&mock);
    }

    #[test]
    #[should_panic(expected = "is not allowed to call this endpoint in restricted mode")]
    fn should_reject_caller_not_in_restricted_set() {
        let unauthorized = Principal::from_slice(&[0xFF]);
        let state = state(Mode::restricted_to(vec![]));

        let mut mock = MockRuntime::new();
        mock.expect_msg_caller().return_const(unauthorized);
        mock.expect_is_controller().return_const(false);

        state.assert_caller_is_allowed(&mock);
    }

    fn state(mode: Mode) -> State {
        State::try_from(dex_types_internal::InitArg { mode }).unwrap()
    }
}
