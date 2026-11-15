# T80 midas (allocator, unsafe): security review
Sampled: heap.rs unsafe blocks (Send impl at 29-30, new and init contracts at 44-65, dealloc contract at 107-120, free-list walk at 152-153). Each unsafe block carries a SAFETY note. dealloc returns early on size 0 or a null pointer.
- No finding in the sampled code. dealloc trusts the caller's pointer by design (documented contract), which is not untrusted input. Not read: buddy.rs (12 unsafe blocks), lock.rs (9), testutil.rs.
Count: high 0, medium 0, low 0.
