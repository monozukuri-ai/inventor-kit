import argparse
import json
from . import read_file, inspect_file

parser = argparse.ArgumentParser(description="Inspect an Inventor part's typed kernel carrier")
parser.add_argument("path")
mode = parser.add_mutually_exclusive_group()
mode.add_argument("--step", help="Export exact supported geometry to this STEP path")
mode.add_argument("--metadata-only", action="store_true", help="Read document properties without decoding geometry")
mode.add_argument("--list-candidates", action="store_true", help="Inventory stored SAB tables without importing Python geometry modules")
parser.add_argument("--candidate-id", help="Select this candidate from this exact input; does not verify current Model State")
parser.add_argument("--require-current-state", action="store_true", help="Refuse geometry unless current Model State is verified")
args = parser.parse_args()
if (args.metadata_only or args.list_candidates) and (args.candidate_id or args.require_current_state):
    parser.error("selection options require reading geometry")
doc = inspect_file(args.path, include_candidates=args.list_candidates) if args.metadata_only or args.list_candidates else read_file(args.path, candidate_id=args.candidate_id, require_current_state=args.require_current_state)
print(json.dumps(doc.summary, ensure_ascii=False, indent=2))
if args.step:
    from cadquery import exporters
    exporters.export(doc.to_cadquery(), args.step)
