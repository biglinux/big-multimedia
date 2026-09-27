#!/usr/bin/env python3
"""Run gtk4-leak-guard with actions attached to the players' content widgets.

Run inside an isolated accessible session, from the workspace root. Arguments
are those of /note/gtk4-leak-guard/sweep/leak_sweep.py.
"""

import importlib.util
import sys

spec = importlib.util.spec_from_file_location(
    "leak_sweep", "/note/gtk4-leak-guard/sweep/leak_sweep.py"
)
sweep = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sweep)
original_step = sweep.run_step


def run_step(step: dict, program: dict, name: str) -> None:
    if step["do"] != "app-action":
        return original_step(step, program, name)
    app = sweep.application(name)
    before = sweep.open_surfaces(app)
    for node in sweep.walk(app):
        action = node.get_action_iface()
        if action is None:
            continue
        for index in range(action.get_n_actions()):
            if action.get_action_name(index) != step["name"]:
                continue
            if not action.do_action(index):
                raise RuntimeError(f"action refused: {step['name']}")
            sweep.time.sleep(step.get("settle", 1))
            after = sweep.open_surfaces(sweep.application(name))
            if after <= before:
                raise RuntimeError(f"no new surface: {before} -> {after}")
            return
    raise RuntimeError(f"missing action: {step['name']}")


sweep.run_step = run_step
sys.exit(sweep.main())
