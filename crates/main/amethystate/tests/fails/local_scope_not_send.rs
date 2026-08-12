use amethystate::LocalScope;

fn assert_send<T: Send>() {}

fn main() {
    assert_send::<LocalScope>();
}
