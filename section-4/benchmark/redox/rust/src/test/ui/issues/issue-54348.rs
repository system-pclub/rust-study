fn main() {
    [1][0u64 as usize];
    [1][1.5 as usize]; //~ ERROR index out of bounds
    //~| ERROR this expression will panic at runtime
    [1][1u64 as usize]; //~ ERROR index out of bounds
    //~| ERROR this expression will panic at runtime
}
