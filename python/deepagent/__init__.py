"""
deepagent — Thin Python wrapper for the Rust deepagent CLI.

Usage:
    from deepagent import run, run_json

    # Simple usage
    result = run("list all .rs files in this project")

    # JSON output with metrics
    data = run_json("explain src/main.rs")
    print(data["result"])
    print(data["metrics"])
"""

import json
import os
import shutil
import subprocess
from pathlib import Path
from typing import Optional


def _find_binary() -> str:
    """Find the deepagent binary."""
    # Check if installed via pip (maturin)
    binary = shutil.which("deepagent")
    if binary:
        return binary

    # Check cargo target directory
    for path in [
        Path(__file__).parent.parent.parent / "target" / "release" / "deepagent",
        Path(__file__).parent.parent.parent / "target" / "debug" / "deepagent",
    ]:
        if path.exists():
            return str(path)

    raise FileNotFoundError(
        "deepagent binary not found. Install with: pip install deepagent"
    )


def run(
    prompt: str,
    *,
    model: Optional[str] = None,
    max_turns: Optional[int] = None,
    timeout: Optional[int] = None,
    verbose: bool = False,
    cwd: Optional[str] = None,
    api_key: Optional[str] = None,
) -> str:
    """
    Run deepagent with a prompt and return the text result.

    Args:
        prompt: The prompt to send to the agent.
        model: Model override (default: gemini-2.5-flash-preview-04-17).
        max_turns: Maximum agent loop iterations.
        timeout: Tool execution timeout in seconds.
        verbose: Show progress on stderr.
        cwd: Working directory for the agent.
        api_key: Gemini API key (defaults to GEMINI_API_KEY env var).

    Returns:
        The agent's text response.
    """
    cmd = [_find_binary(), "-p", prompt]

    if model:
        cmd.extend(["--model", model])
    if max_turns is not None:
        cmd.extend(["--max-turns", str(max_turns)])
    if timeout is not None:
        cmd.extend(["--timeout", str(timeout)])
    if verbose:
        cmd.append("--verbose")

    env = os.environ.copy()
    if api_key:
        env["GEMINI_API_KEY"] = api_key

    result = subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        cwd=cwd,
        env=env,
        timeout=timeout or 300,
    )

    if result.returncode != 0:
        raise RuntimeError(
            f"deepagent failed (exit {result.returncode}): {result.stderr}"
        )

    return result.stdout.strip()


def run_json(
    prompt: str,
    *,
    model: Optional[str] = None,
    max_turns: Optional[int] = None,
    timeout: Optional[int] = None,
    cwd: Optional[str] = None,
    api_key: Optional[str] = None,
) -> dict:
    """
    Run deepagent with JSON output and return structured results.

    Returns a dict with keys: result, metrics, model, session_id, events.
    """
    cmd = [_find_binary(), "--json", "-p", prompt]

    if model:
        cmd.extend(["--model", model])
    if max_turns is not None:
        cmd.extend(["--max-turns", str(max_turns)])
    if timeout is not None:
        cmd.extend(["--timeout", str(timeout)])

    env = os.environ.copy()
    if api_key:
        env["GEMINI_API_KEY"] = api_key

    result = subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        cwd=cwd,
        env=env,
        timeout=timeout or 300,
    )

    if result.returncode != 0:
        raise RuntimeError(
            f"deepagent failed (exit {result.returncode}): {result.stderr}"
        )

    return json.loads(result.stdout)


__version__ = "0.9.1"
__all__ = ["run", "run_json", "__version__"]
