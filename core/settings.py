"""Per-user settings and the directory runs write into.

Kept in LOCALAPPDATA rather than beside the executable so the program works
from a read-only or Program Files install, and so a rebuild does not throw
away the settings. Roch Viewer stores its settings the same way.
"""

import json
import os

APP_DIR_NAME = "RochStressTest"


def app_data_dir():
    """The per-user directory this program owns, created on demand."""
    base = os.environ.get("LOCALAPPDATA") or os.path.expanduser("~")
    path = os.path.join(base, APP_DIR_NAME)
    os.makedirs(path, exist_ok=True)
    return path


def run_dir(name):
    """A scratch directory for one tool's run artifacts.

    Prime95 in particular insists on writing prime.txt, local.txt and
    results.txt somewhere; pointing it here keeps its state out of the
    distribution folder, which stays exactly as it was unpacked.
    """
    path = os.path.join(app_data_dir(), "runs", name)
    os.makedirs(path, exist_ok=True)
    return path


def log_dir():
    """Where finished-run transcripts are kept."""
    path = os.path.join(app_data_dir(), "logs")
    os.makedirs(path, exist_ok=True)
    return path


def settings_path():
    return os.path.join(app_data_dir(), "settings.json")


def load_settings():
    """Return the saved settings, or an empty dict when there are none."""
    try:
        with open(settings_path(), "r", encoding="utf-8") as handle:
            saved = json.load(handle)
            return saved if isinstance(saved, dict) else {}
    except (OSError, ValueError, TypeError):
        return {}


def save_setting(key, value):
    """Merge one setting into the file, leaving the others alone."""
    settings = load_settings()
    settings[key] = value
    try:
        with open(settings_path(), "w", encoding="utf-8") as handle:
            json.dump(settings, handle, indent=2)
    except OSError as error:
        print(f"Could not save setting {key}: {error}")


def load_appearance_mode():
    mode = load_settings().get("appearance_mode", "Dark")
    if str(mode).lower() in ("light", "dark"):
        return str(mode).title()
    return "Dark"
