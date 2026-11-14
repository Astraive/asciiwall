# Resource ownership, concurrency, and shutdown

## Ownership model

The controller owns saved configuration, package installs, permission policy, assignments, and supervision. A native host owns its event loop, surface leases, GPU device resources, and scene runtime. A restricted media worker owns decoder state and approved input handles. The UI owns drafts and preview requests, not active native handles.

Core frames use owned/shared backing buffers with explicit lifetime. A borrowed raster view cannot outlive its buffer. A submitted GPU draw keeps its atlas/textures alive until the relevant GPU work completes. A Rust reference count alone does not prove the GPU has finished using a native allocation.

## Bounded handoff

Use bounded control/data queues and a fixed shared-buffer pool. Each slot follows `Free -> Filling -> Published -> Consumed -> Released -> Free`. Before exposing bytes, the consumer validates descriptor generation and publication identity, dimensions, row stride, format, byte length, and mapped offset against the local pool and core raster contract. Stale frames are released rather than accumulated for catch-up. A consumed slot cannot be reused until the consumer explicitly releases it after CPU/native/GPU work completes; a Rust reference count or dropped lease does not establish device completion. Generation invalidation may reclaim old slots only after the old mapped region is unavailable to its transport/native owner.

Control events and frame data use separate queues so a flood of frames cannot block Stop or permission revocation. High-rate motion events coalesce; important state changes remain ordered. Avoid holding a settings mutex while waiting for a decoder or presenting a GPU surface.

## Thread rules

Native APIs have platform thread constraints, especially AppKit and shell-hosted render loops. [S52](../research/source-register.md#s52) [S56](../research/source-register.md#s56) Keep those owners explicit rather than moving every resource into a generic async task. CPU conversion uses a bounded worker pool; additional decoders/outputs share a global concurrency budget.

Prefer message passing between owners. Any shared mutable cache has a documented locking order and cannot call back into UI/native code while locked. Callback handlers enqueue small messages and return quickly.

In `ascii-runtime`, the `BoundedHandoff` operation lock is outermost. Queue locks are released before pool operations, and queue and pool locks are never held simultaneously. Completion signals are atomic. `FrameLease::with_pixels` callbacks run under the pool lock and MUST be short CPU-only work; they MUST NOT call UI/native APIs, re-enter the pool, or wait for decode/presentation. Never hold a settings lock while waiting for decode or presentation. `ResourceBudget` shares CPU, decode, output, and CPU/GPU-memory permits across clones; transition reservations charge old and new resources together and release the old charge only when transition completion is recorded.

## Cancellation hierarchy

Application -> assignment -> source/decode job -> frame/conversion/export job. Cancelling a parent invalidates descendant generations and prevents new work. A job may finish a small noninterruptible native operation, but its output is discarded when stale. A hung worker is terminated by the supervisor after a documented timeout.

Permission revocation cancels observer work immediately and clears sensitive history; it does not wait for a visual transition. Stop has higher priority than cosmetic scene events.

## Shutdown sequence

Stop accepting commands; mark assignments stopping; cancel nonessential jobs; unsubscribe observers; request host detach on the appropriate native thread; drain or abandon bounded in-flight buffers safely; terminate unresponsive workers; persist already validated state; close IPC and controller resources.

Normal exit may run a short transition if healthy, but shutdown/lock/crash paths do not guarantee one. Do not block logout waiting for media export or GPU completion indefinitely. A host detects controller-channel loss and leaves the desktop in a safe state.

## Leak and race tests

Repeated Apply/Stop, font changes during transition, output hotplug, media-worker crash, device loss, cancelled export, and controller death must return resource counts to baseline within the defined grace period. Track owned native windows, threads, decoder instances, shared buffers, atlas pages, and file handles in diagnostic builds.

Memory budgeting counts simultaneous old/new scenes during transitions and both CPU/GPU copies. An LRU cache entry with an active lease cannot be freed until consumers release it, but cache ownership must not keep a stopped wallpaper alive forever.
