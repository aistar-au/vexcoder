# Introduction

VexCoder is a local coding assistant you run as a binary. It connects to the
model endpoint you configure, supports interactive CLI sessions and
non-interactive batch runs, and keeps its setup lightweight enough to build
from source on supported platforms.

This book focuses on the public user surface:

- building the binary
- understanding the current runtime, application, and transport layout
- creating a workspace with `vex init`
- configuring the model endpoint and token
- using the current CLI flags and interactive commands

The shortest path to a running session is in [Quick Start](quick-start.md). For the current code layout, see [Architecture Overview](architecture.md).
