# Incremental Compilation In Detail

The incremental compilation scheme is, in essence, a surprisingly
simple extension to the overall query system. It relies on the fact that:

  1. queries are pure functions -- given the same inputs, a query will always
     yield the same result, and
  2. the query model structures compilation in an acyclic graph that makes
     dependencies between individual computations explicit.

This chapter will explain how we can use these properties for making things
incremental and then goes on to discuss version implementation issues.

# A Basic Algorithm For Incremental Query Evaluation

As explained in the [query evaluation model primer][query-model], query
invocations form a directed-acyclic graph. Here's the example from the
previous chapter again:

```ignore
  list_of_all_hir_items <----------------------------- type_check_crate()
                                                               |
                                                               |
  Hir(foo) <--- type_of(foo) <--- type_check_item(foo) <-------+
                                      |                        |
                    +-----------------+                        |
                    |                                          |
                    v                                          |
  Hir(bar) <--- type_of(bar) <--- type_check_item(bar) <-------+
```

Since every access from one query to another has to go through the query
context, we can record these accesses and thus actually build this dependency
graph in memory. With dependency tracking enabled, when compilation is done,
we know which queries were invoked (the nodes of the graph) and for each
invocation, which other queries or input has gone into computing the query's
result (the edges of the graph).

Now suppose, we change the source code of our program so that
HIR of `bar` looks different than before. Our goal is to only recompute
those queries that are actually affected by the change while just re-using
the cached results of all the other queries. Given the dependency graph we can
do exactly that. For a given query invocation, the graph tells us exactly
what data has gone into computing its results, we just have to follow the
edges until we reach something that has changed. If we don't encounter
anything that has changed, we know that the query still would evaluate to
the same result we already have in our cache.

Taking the `type_of(foo)` invocation from above as example, we can check
whether the cached result is still valid by following the edges to its
inputs. The only edge leads to `Hir(foo)`, an input that has not been affected
by the change. So we know that the cached result for `type_of(foo)` is still
valid.

The story is a bit different for `type_check_item(foo)`: We again walk the
edges and already know that `type_of(foo)` is fine. Then we get to
`type_of(bar)` which we have not checked yet, so we walk the edges of
`type_of(bar)` and encounter `Hir(bar)` which *has* changed. Consequently
the result of `type_of(bar)` might yield a different same result than what we
have in the cache and, transitively, the result of `type_check_item(foo)`
might have changed too. We thus re-run `type_check_item(foo)`, which in
turn will re-run `type_of(bar)`, which will yield an up-to-date result
because it reads the up-to-date version of `Hir(bar)`.


# The Problem With The Basic Algorithm: False Positives

If you read the previous paragraph carefully, you'll notice that it says that
`type_of(bar)` *might* have changed because one of its inputs has changed.
There's also the possibility that it might still yield exactly the same
result *even though* its input has changed. Consider an example with a
simple query that just computes the sign of an integer:

```ignore
  IntValue(x) <---- sign_of(x) <--- some_other_query(x)
```

Let's say that `IntValue(x)` starts out as `1000` and then is set to `2000`.
Even though `IntValue(x)` is different in the two cases, `sign_of(x)` yields
the result `+` in both cases.

If we follow the basic algorithm, however, `some_other_query(x)` would have to
(unnecessarily) be re-evaluated because it transitively depends on a changed
input. Change detection yields a "false positive" in this case because it has
to conservatively assume that `some_other_query(x)` might be affected by that
changed input.

Unfortunately it turns out that the actual queries in the compiler are full
of examples like this and small changes to the input often potentially affect
very large parts of the output binaries. As a consequence, we had to make the
change detection system smarter and more accurate.

# Improving Accuracy: The red-green Algorithm

The "false positives" problem can be solved by interleaving change detection
and query re-evaluation. Instead of walking the graph all the way to the
inputs when trying to find out if some cached result is still valid, we can
check if a result has *actually* changed after we were forced to re-evaluate
it.

We call this algorithm, for better or worse, the red-green algorithm because nodes
in the dependency graph are assigned the color green if we were able to prove
that its cached result is still valid and the color red if the result has
turned out to be different after re-evaluating it.

The meat of red-green change tracking is implemented in the try-mark-green
algorithm, that, you've guessed it, tries to mark a given node as green:

```rust,ignore
fn try_mark_green(tcx, current_node) -> bool {

    // Fetch the inputs to `current_node`, i.e. get the nodes that the direct
    // edges from `node` lead to.
    let dependencies = tcx.dep_graph.get_dependencies_of(current_node);

    // Now check all the inputs for changes
    for dependency in dependencies {

        match tcx.dep_graph.get_node_color(dependency) {
            Green => {
                // This input has already been checked before and it has not
                // changed; so we can go on to check the next one
            }
            Red => {
                // We found an input that has changed. We cannot mark
                // `current_node` as green without re-running the
                // corresponding query.
                return false
            }
            Unknown => {
                // This is the first time we are look at this node. Let's try
                // to mark it green by calling try_mark_green() recursively.
                if try_mark_green(tcx, dependency) {
                    // We successfully marked the input as green, on to the
                    // next.
                } else {
                    // We could *not* mark the input as green. This means we
                    // don't know if its value has changed. In order to find
                    // out, we re-run the corresponding query now!
                    tcx.run_query_for(dependency);

                    // Fetch and check the node color again. Running the query
                    // has forced it to either red (if it yielded a different
                    // result than we have in the cache) or green (if it
                    // yielded the same result).
                    match tcx.dep_graph.get_node_color(dependency) {
                        Red => {
                            // The input turned out to be red, so we cannot
                            // mark `current_node` as green.
                            return false
                        }
                        Green => {
                            // Re-running the query paid off! The result is the
                            // same as before, so this particular input does
                            // not invalidate `current_node`.
                        }
                        Unknown => {
                            // There is no way a node has no color after
                            // re-running the query.
                            panic!("unreachable")
                        }
                    }
                }
            }
        }
    }

    // If we have gotten through the entire loop, it means that all inputs
    // have turned out to be green. If all inputs are unchanged, it means
    // that the query result corresponding to `current_node` cannot have
    // changed either.
    tcx.dep_graph.mark_green(current_node);

    true
}

// Note: The actual implementation can be found in
//       src/librustc/dep_graph/graph.rs
```

By using red-green marking we can avoid the devastating cumulative effect of
having false positives during change detection. Whenever a query is executed
in incremental mode, we first check if its already green. If not, we run
`try_mark_green()` on it. If it still isn't green after that, then we actually
invoke the query provider to re-compute the result.



# The Real World: How Persistence Makes Everything Complicated

The sections above described the underlying algorithm for incremental
compilation but because the compiler process exits after being finished and
takes the query context with its result cache with it into oblivion, we have
persist data to disk, so the next compilation session can make use of it.
This comes with a whole new set of implementation challenges:

- The query results cache is stored to disk, so they are not readily available
  for change comparison.
- A subsequent compilation session will start off with new version of the code
  that has arbitrary changes applied to it. All kinds of IDs and indices that
  are generated from a global, sequential counter (e.g. `NodeId`, `DefId`, etc)
  might have shifted, making the persisted results on disk not immediately
  usable anymore because the same numeric IDs and indices might refer to
  completely new things in the new compilation session.
- Persisting things to disk comes at a cost, so not every tiny piece of
  information should be actually cached in between compilation sessions.
  Fixed-sized, plain-old-data is preferred to complex things that need to run
  branching code during (de-)serialization.

The following sections describe how the compiler currently solves these issues.

## A Question Of Stability: Bridging The Gap Between Compilation Sessions

As noted before, various IDs (like `DefId`) are generated by the compiler in a
way that depends on the contents of the source code being compiled. ID assignment
is usually deterministic, that is, if the exact same code is compiled twice,
the same things will end up with the same IDs. However, if something
changes, e.g. a function is added in the middle of a file, there is no
guarantee that anything will have the same ID as it had before.

As a consequence we cannot represent the data in our on-disk cache the same
way it is represented in memory. For example, if we just stored a piece
of type information like `TyKind::FnDef(DefId, &'tcx Substs<'tcx>)` (as we do
in memory) and then the contained `DefId` points to a different function in
a new compilation session we'd be in trouble.

The solution to this problem is to find "stable" forms for IDs which remain
valid in between compilation sessions. For the most important case, `DefId`s,
these are the so-called `DefPath`s. Each `DefId` has a
corresponding `DefPath` but in place of a numeric ID, a `DefPath` is based on
the path to the identified item, e.g. `std::collections::HashMap`. The
advantage of an ID like this is that it is not affected by unrelated changes.
For example, one can add a new function to `std::collections` but
`std::collections::HashMap` would still be `std::collections::HashMap`. A
`DefPath` is "stable" across changes made to the source code while a `DefId`
isn't.

There is also the `DefPathHash` which is just a 128-bit hash value of the
`DefPath`. The two contain the same information and we mostly use the
`DefPathHash` because it simpler to handle, being `Copy` and self-contained.

This principle of stable identifiers is used to make the data in the on-disk
cache resilient to source code changes. Instead of storing a `DefId`, we store
the `DefPathHash` and when we deserialize something from the cache, we map the
`DefPathHash` to the corresponding `DefId` in the *current* compilation session
(which is just a simple hash table lookup).

The `HirId`, used for identifying HIR components that don't have their own
`DefId`, is another such stable ID. It is (conceptually) a pair of a `DefPath`
and a `LocalId`, where the `LocalId` identifies something (e.g. a `hir::Expr`)
locally within its "owner" (e.g. a `hir::Item`). If the owner is moved around,
the `LocalId`s within it are still the same.



## Checking Query Results For Changes: StableHash And Fingerprints

In order to do red-green-marking we often need to check if the result of a
query has changed compared to the result it had during the previous
compilation session. There are two performance problems with this though:

- We'd like to avoid having to load the previous result from disk just for
  doing the comparison. We already computed the new result and will use that.
  Also loading a result from disk will "pollute" the interners with data that
  is unlikely to ever be used.
- We don't want to store each and every result in the on-disk cache. For
  example, it would be wasted effort to persist things to disk that are
  already available in upstream crates.

The compiler avoids these problems by using so-called `Fingerprint`s. Each time
a new query result is computed, the query engine will compute a 128 bit hash
value of the result. We call this hash value "the `Fingerprint` of the query
result". The hashing is (and has to be) done "in a stable way". This means
that whenever something is hashed that might change in between compilation
sessions (e.g. a `DefId`), we instead hash its stable equivalent
(e.g. the corresponding `DefPath`). That's what the whole `StableHash`
infrastructure is for. This way `Fingerprint`s computed in two
different compilation sessions are still comparable.

The next step is to store these fingerprints along with the dependency graph.
This is cheap since fingerprints are just bytes to be copied. It's also cheap to
load the entire set of fingerprints together with the dependency graph.

Now, when red-green-marking reaches the point where it needs to check if a
result has changed, it can just compare the (already loaded) previous
fingerprint to the fingerprint of the new result.

This approach works rather well but it's not without flaws:

- There is a small possibility of hash collisions. That is, two different
  results could have the same fingerprint and the system would erroneously
  assume that the result hasn't changed, leading to a missed update.

  We mitigate this risk by using a high-quality hash function and a 128 bit
  wide hash value. Due to these measures the practical risk of a hash
  collision is negligible.

- Computing fingerprints is quite costly. It is the main reason why incremental
  compilation can be slower than non-incremental compilation. We are forced to
  use a good and thus expensive hash function, and we have to map things to
  their stable equivalents while doing the hashing.

In the future we might want to explore different approaches to this problem.
For now it's `StableHash` and `Fingerprint`.



## A Tale Of Two DepGraphs: The Old And The New

The initial description of dependency tracking glosses over a few details
that quickly become a head scratcher when actually trying to implement things.
In particular it's easy to overlook that we are actually dealing with *two*
dependency graphs: The one we built during the previous compilation session and
the one that we are building for the current compilation session.

When a compilation session starts, the compiler loads the previous dependency
graph into memory as an immutable piece of data. Then, when a query is invoked,
it will first try to mark the corresponding node in the graph as green. This
means really that we are trying to mark the node in the *previous* dep-graph
as green that corresponds to the query key in the *current* session. How do we
do this mapping between current query key and previous `DepNode`? The answer
is again `Fingerprint`s: Nodes in the dependency graph are identified by a
fingerprint of the query key. Since fingerprints are stable across compilation
sessions, computing one in the current session allows us to find a node
in the dependency graph from the previous session. If we don't find a node with
the given fingerprint, it means that the query key refers to something that
did not yet exist in the previous session.

So, having found the dep-node in the previous dependency graph, we can look
up its dependencies (also dep-nodes in the previous graph) and continue with
the rest of the try-mark-green algorithm. The next interesting thing happens
when we successfully marked the node as green. At that point we copy the node
and the edges to its dependencies from the old graph into the new graph. We
have to do this because the new dep-graph cannot not acquire the
node and edges via the regular dependency tracking. The tracking system can
only record edges while actually running a query -- but running the query,
although we have the result already cached, is exactly what we want to avoid.

Once the compilation session has finished, all the unchanged parts have been
copied over from the old into the new dependency graph, while the changed parts
have been added to the new graph by the tracking system. At this point, the
new graph is serialized out to disk, alongside the query result cache, and can
act as the previous dep-graph in a subsequent compilation session.


## Didn't You Forget Something?: Cache Promotion
TODO


# The Future: Shortcomings Of The Current System and Possible Solutions
TODO


[query-model]: ./query-evaluation-model-in-detail.html
