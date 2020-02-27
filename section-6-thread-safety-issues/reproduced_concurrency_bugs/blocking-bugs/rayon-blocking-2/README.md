# rayon-449

This is a blocking bug.

There are three threads involved in this deadlock. One is the main thread. It creates two thread-pools and submits one job to the first thread-pool. The other two are worker threads inside the two thread-pools. 

When a job is submitted to a thread pool, the submitting thread creates a StackJob object and blocks itself by invoking StackJob.latch.wait(). When a submitted job is finished by one of the worker threads inside the thread-pool, StackJob.latch.set() is invoked to unblock the submitting thread. 

There are three jobs created, Job A, Job B, and Job C. Job A is submitted the first thread-pool, and it is to submit Job B to the second thread-pool. Job B is to submit Job C to the first thread pool. 

When the main thread submits Job A to the first thread-pool, it blocks itself and waits for the worker thread inside the first thread-pool to finish the Job A. When the worker thread inside the first thread-pool processes Job A, it submits Job B to the second thread-pool and waits for the worker thread inside the second thread-pool to process Job B. When the worker thread inside the second thread-pool processes Job B, it submits Job C to the first thread-pool and wait for any worker thread inside the first thread-pool to process Job C. However, there is no available worker thread inside the first thread-pool. 

