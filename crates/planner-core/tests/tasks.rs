use planner_core::Store;

#[test]
fn tasks_subtasks_and_hiding_done() {
    let s = Store::open_in_memory().unwrap();
    let a = s.add_task("  Buy groceries ", None).unwrap();
    assert_eq!(a.title, "Buy groceries");
    std::thread::sleep(std::time::Duration::from_millis(2));
    let b = s.add_task("Plan trip", None).unwrap();
    let milk = s.add_task("Milk", Some(&a.id)).unwrap();
    s.add_task("Eggs", Some(&a.id)).unwrap();
    assert!(s.add_task("Nested", Some(&milk.id)).is_err());
    assert!(s.add_task("   ", None).is_err());

    // Newest first; subtasks in the order added.
    let list = s.tasks(false).unwrap();
    assert_eq!(list.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), ["Plan trip", "Buy groceries"]);
    assert_eq!(list[1].subtasks.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), ["Milk", "Eggs"]);

    s.update_task(&b.id, "Plan the trip", Some("Flights\nHotel\n\n")).unwrap();
    s.update_task(&a.id, "Buy groceries", Some("   ")).unwrap();
    let t = s.task(&b.id).unwrap().unwrap();
    assert_eq!(t.notes.as_deref(), Some("Flights\nHotel"));
    assert_eq!(s.task(&a.id).unwrap().unwrap().notes, None);

    // Done tasks are hidden unless asked for.
    s.set_task_done(&a.id, true).unwrap();
    assert_eq!(s.tasks(false).unwrap().len(), 1);
    assert_eq!(s.done_task_count().unwrap(), 1);
    let all = s.tasks(true).unwrap();
    assert_eq!(all.len(), 2);
    assert!(all[1].done && all[1].done_at.is_some());
    s.set_task_done(&a.id, false).unwrap();
    assert_eq!(s.tasks(false).unwrap().len(), 2);

    // Deleting a task takes its subtasks with it.
    s.delete_task(&a.id).unwrap();
    assert!(s.task(&milk.id).unwrap().is_none());
    assert_eq!(s.tasks(true).unwrap().len(), 1);
}

#[test]
fn task_changes_are_queued_for_sync() {
    let s = Store::open_in_memory().unwrap();
    let t = s.add_task("Call mum", None).unwrap();
    s.set_task_done(&t.id, true).unwrap();
    let pending = s.sync_pending(10).unwrap();
    let row = pending.iter().find(|p| p.table == "tasks").expect("queued");
    assert_eq!(row.row["title"], "Call mum");
    assert_eq!(row.row["done"], 1);
}
