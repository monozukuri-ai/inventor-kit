"""Compare saved candidate observations, optionally with same-input vendor captures."""
from __future__ import annotations
import argparse
from dataclasses import asdict
import hashlib
import json
from pathlib import Path

import inventor_kit
from oracle_contract import load_capture, compare_capture, shape_metrics


def compare_file(path, capture_path=None):
    data=Path(path).read_bytes();sha=hashlib.sha256(data).hexdigest()
    capture=load_capture(capture_path,source_sha256=sha) if capture_path else None
    doc=inventor_kit.inspect(data,source_id=str(path),include_candidates=True)
    candidates=[]
    for c in doc.geometry.candidates:
        metrics=None;error=None
        if capture is not None:
            try:
                selected=inventor_kit.read(data,source_id=str(path),candidate_id=c.id)
                shape=selected.to_cadquery().val()
                if not shape.isValid():raise ValueError('Converted shape is invalid')
                metrics=shape_metrics([shape])
            except Exception as e:error=f'{type(e).__name__}: {e}'
        candidates.append(dict(id=c.id,kernel_sha256=c.kernel_sha256,segment_id=c.segment_id,record_ordinal=c.record_ordinal,
            header=asdict(c.header) if c.header else None,carrier=asdict(c.carrier) if c.carrier else None,
            meta_state_words=doc.geometry.segments[c.segment_index].meta_state_words,
            table_status=c.table_status,profile_status=c.profile_status,conversion_error=error,
            comparison=compare_capture(capture,metrics) if capture is not None else None))
    return dict(file=str(path),sha256=sha,inventory_status=doc.geometry.status,
                captured_model_state=capture['capture']['model_state'] if capture else None,
                capture_provider=capture['provider'] if capture else None,
                candidates=candidates,state_verification='unverified',
                matching_metric_candidates=[c['id'] for c in candidates if c['comparison'] and c['comparison']['status']=='matched_metrics'])


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('files',nargs='+',type=Path)
    parser.add_argument('--capture',nargs=2,action='append',default=[],metavar=('SOURCE','CAPTURE'))
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    captures={}
    inputs={p.resolve() for p in args.files}
    for source,capture in args.capture:
        key=Path(source).resolve()
        if key not in inputs or key in captures:parser.error('Capture requires one unique source from the input list')
        captures[key]=Path(capture)
    results=[compare_file(p,captures.get(p.resolve())) for p in args.files]
    report=dict(scope='Saved candidate observations; input relationship and active-state ownership are not inferred',
                results=results,
                kernel_observations=[dict(file=r['file'],sha256=r['sha256'],kernels=[c['kernel_sha256'] for c in r['candidates']]) for r in results],
                limitations=['Equal aggregate metrics do not establish a Model State binding, face orientation or body identity.',
                             'Changed fields in differently saved files do not by themselves identify suppression, state selectors or delta semantics.',
                             'Candidate IDs are scoped to each source SHA-256; segment/ordinal equality across files is not an identity guarantee.'])
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(report,ensure_ascii=False,indent=2,allow_nan=False)+'\n')


if __name__=='__main__':main()
