#lazy-static.rs

When the input closure of `Once.call_once()` recursively call `Once.call_once()` of the same Once object, 
a deadlock or a panic can be triggered. 
