use super::*;
use ratatui::{backend::TestBackend, Terminal};

#[test]
fn full_access_requires_a_second_confirmation() {
    let mut picker = PermissionPickerState::new(UserPermissionMode::AskForApproval);
    picker.move_selection(1);
    picker.move_selection(1);

    assert_eq!(picker.confirm(), None);
    assert_eq!(picker.confirm(), Some(UserPermissionMode::FullAccess));
}

#[test]
fn renders_the_canonical_permission_picker_title_and_modes() {
    let picker = PermissionPickerState::new(UserPermissionMode::ApproveForMe);
    let backend = TestBackend::new(72, 14);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| picker.render(frame, frame.area()))
        .unwrap();

    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("Choose what Talos is allowed to do"));
    assert!(text.contains("Ask for approval"));
    assert!(text.contains("Approve for me (current)"));
    assert!(text.contains("Full access"));
}
