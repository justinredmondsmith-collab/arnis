# Coastal repair scope and decision gates

Maintainer authorization: stay with our app; create/red-team a plan and start execution. Issue renderer #27, app #90. Retain upstream v3.1.0 and the qualified master-coordinate architecture. Java 1.20.1 is the target; user owns Minecraft tests.

Desired outcome: the original failed coastal area builds in the updated GUI with correct shoreline, real dry land and infrastructure retained, common sea height across tiles, and preserved independent inland waters. No threshold bump, invented ocean closure, bbox shrink or per-tile water decision.

Three alternatives: (A) fix an evidenced acquisition/classification/repair defect, preferred if demonstrated; (B) explicitly revise the conservative coastal contract if sound inputs expose an invalid assumption, with new provenance and both consumer/producer changes; (C) raise the threshold or suppress the guard, rejected because it may flatten real land. We cannot select A versus B before the full repaired-grid diagnosis.

First implementation checkpoint is diagnostics only: exact frozen-input failure replay and test-only characterization using the real AWS sampler and existing repair functions. No runtime behavior changes before the real-data findings and refined fix design are reviewed. This is a deliberate staged design, not a claim that a final patch is already known. Proceed under the user's instruction to plan, review and execute without another routine permission request.
