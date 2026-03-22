use crate::App;

#[test]
fn toggle_input_monitoring() {
    let mut app = App::new_headless();
    assert!(!app.input_monitoring, "monitoring should start disabled");
    app.input_monitoring = true;
    assert!(app.input_monitoring);
    app.input_monitoring = false;
    assert!(!app.input_monitoring);
}
