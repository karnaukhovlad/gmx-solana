# GMX-Solana GLV Market-Limit Investigation

This repository contains an investigation into the practical number of markets
that a GMX-Solana GLV transaction can process.

## Documentation

The [`docs/`](docs/) directory contains the detailed AI investigation record. Start
with the [GLV market-limit documentation](docs/glv-market-limit/README.md), which
links to:

- the consolidated root-cause analysis;
- account-count formulas and reproduction results;
- solution proposals and trade-offs;
- an audit risk register;
- numbered investigation iterations that preserve the analysis history.

The investigation concludes that the observed limit is caused by Solana's
64-account transaction lock limit, rather than a fixed 12-market limit in the
GMX-Solana program.

## Report

[`REPORT.md`](REPORT.md) is the concise submission report. It summarizes the root
cause, reproduction method, measured deposit and withdrawal boundaries,
recommended solution, alternatives considered, and reflections on the use of AI
during the investigation.

Supporting test output is available in the [`report/`](report/) directory for
local validator runs with 64-account and 128-account limits.
