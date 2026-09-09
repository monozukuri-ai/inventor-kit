"""Prepare isolated IPT state saves and capture them with full Inventor on Windows.

This collector is not yet qualified on Autodesk. It supports self-contained parts
only, never saves the input, and refuses to substitute Apprentice for Model States.
"""
from datetime import datetime, timezone
from pathlib import Path
import argparse
import hashlib
import json
import shutil
import sys

from capture_vendor_oracle import collect_document, collect_geometry, items
from oracle_contract import validate_capture


def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def active_name(doc):
    return str(doc.ComponentDefinition.ModelStates.ActiveModelState.Name)


def check_document(doc):
    if int(doc.DocumentType)!=12290:raise ValueError('This collector supports part documents only')
    if doc.File.ReferencedFileDescriptors.Count:
        raise ValueError('External references need a qualified copied dependency set; this collector requires self-contained IPT files')


def select_and_save(doc, name):
    check_document(doc)
    doc.ComponentDefinition.ModelStates.Item(name).Activate()
    if active_name(doc)!=name:raise ValueError('Requested Model State did not activate')
    if not doc.Update2(False):raise ValueError('Inventor failed to update the selected state')
    if active_name(doc)!=name or doc.RequiresUpdate:raise ValueError('State/update changed during preparation')
    doc.Save2(False)  # Save this isolated copy, never dependent documents.


def suppression_observations(doc):
    result=[]
    for feature in items(doc.ComponentDefinition.Features):
        entry=dict(name=str(feature.Name),suppressed=None,status='unavailable')
        try:entry.update(suppressed=bool(feature.Suppressed),status='captured')
        except Exception as error:entry['reason']=str(error)
        result.append(entry)
    return result


def capture(input_path, output_dir, requested_states=None):
    if sys.platform!='win32':raise RuntimeError('Requires Windows, pywin32 and full Autodesk Inventor; no Apprentice fallback')
    import win32com.client
    input_path=input_path.resolve();original_sha=sha(input_path)
    output_dir=output_dir.resolve();output_dir.mkdir(parents=True,exist_ok=False)
    app=win32com.client.DispatchEx('Inventor.Application')
    records=[]
    try:
        app.SilentOperation=True
        discovery=output_dir/'discovery.ipt';shutil.copy2(input_path,discovery)
        doc=app.Documents.Open(str(discovery),False)
        try:
            check_document(doc)
            names=[str(s.Name) for s in items(doc.ComponentDefinition.ModelStates)]
        finally:doc.Close(True)
        if len(names)!=len(set(names)):raise ValueError('Duplicate Model State names')
        states=names if requested_states is None else requested_states
        if not states or len(states)!=len(set(states)) or any(s not in names for s in states):
            raise ValueError('State list is empty, duplicate or unknown')
        version=app.SoftwareVersion.DisplayName+' / '+str(app.SoftwareVersion.BuildIdentifier)
        for index,name in enumerate(states):
            folder=output_dir/f'state-{index:03d}';folder.mkdir()
            saved=folder/'part.ipt';shutil.copy2(input_path,saved)
            doc=app.Documents.Open(str(saved),False)
            try:select_and_save(doc,name)
            finally:doc.Close(True)
            # The oracle belongs to the bytes AFTER explicit preparation. Reopen
            # without Update/Save; recheck the saved state's identity and hash.
            snapshot_sha=sha(saved);snapshot_bytes=saved.stat().st_size
            doc=app.Documents.Open(str(saved),False)
            try:
                check_document(doc)
                if active_name(doc)!=name or doc.RequiresUpdate or doc.Dirty:
                    raise ValueError('Reopened snapshot is dirty, stale or in a different active state')
                observation=dict(schema_version=1,
                    source=dict(sha256=snapshot_sha,bytes=snapshot_bytes,file_name=saved.name),
                    provider=dict(name='autodesk.inventor',version=version,platform='Windows',collector_sha256=sha(__file__)),
                    capture=dict(acquired_at=datetime.now(timezone.utc).isoformat(),scope='active_model_state',model_state=name,source_modified=False,update_performed=False),
                    document=collect_document(doc),geometry=collect_geometry(doc),
                    limitations=['Collector execution must be qualified on a real Autodesk installation.',
                                 'Preparation activated, updated and saved a separate IPT copy; measurement reopens those exact bytes without updating.',
                                 'Native geometry metrics do not identify a kernel candidate or prove face-normal correspondence.'])
                suppressed=suppression_observations(doc)
                if active_name(doc)!=name or doc.RequiresUpdate or doc.Dirty:raise ValueError('Snapshot state changed during measurement')
            finally:doc.Close(True)
            if sha(saved)!=snapshot_sha or sha(input_path)!=original_sha:raise ValueError('Input or snapshot bytes changed during measurement')
            validate_capture(observation,source_sha256=snapshot_sha)
            capture_path=folder/'capture.json'
            capture_path.write_text(json.dumps(observation,ensure_ascii=False,indent=2,allow_nan=False)+'\n',encoding='utf-8')
            records.append(dict(model_state=name,file=str(saved.relative_to(output_dir)),sha256=snapshot_sha,
                                capture=str(capture_path.relative_to(output_dir)),capture_sha256=sha(capture_path),
                                preparation_update_performed=True,suppression=suppressed))
        manifest=dict(schema_version=1,source=dict(file_name=input_path.name,sha256=original_sha),
                      scope='prepared IPT state snapshots; candidate-to-state binding remains unverified',
                      collector_files={n:sha(Path(__file__).with_name(n)) for n in ['capture_model_states.py','capture_vendor_oracle.py','oracle_contract.py']},states=records)
        (output_dir/'states.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2,allow_nan=False)+'\n',encoding='utf-8')
        return manifest
    finally:
        app.Quit()
        if sha(input_path)!=original_sha:raise ValueError('Original input changed')


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input',type=Path,required=True)
    parser.add_argument('--output-dir',type=Path,required=True,help='New directory; existing directories are refused')
    parser.add_argument('--state',action='append',help='Exact state name; repeat or omit to enumerate all')
    args=parser.parse_args();capture(args.input,args.output_dir,args.state)


if __name__=='__main__':main()
