fn main() {
    let _: Option<()> = do catch {};
    //~^ ERROR found removed `do catch` syntax
    //~^^ HELP following RFC #2388, the new non-placeholder syntax is `try`
}
