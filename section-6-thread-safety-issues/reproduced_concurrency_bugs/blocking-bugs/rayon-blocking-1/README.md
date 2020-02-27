#rayon-f17d745e12511516396f6ba5027a30e88e981bfb

This is a blocking bug related to missing `Condvar` `notify`.

The buggy version implements latch using an atomic bool variable (b), a mutex without any shared variable to protect (m), and a conditional variable (v). The blocking functionality inside `Latch.wait()` is achieved through `Condvar.wait()`. Inside `Latch.set()`, `Convar.notify_all()` is called to awake all blocking threads. 

There is a possible interleave, where a thread can miss `Convar.notify_all()` and block for ever (block for a while unnecessarily).

run `./install.sh` before `cargo run`
