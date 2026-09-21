# Windows PowerShell 5.1 / Inventor. Observe newly saved drawings without reopening.
# API: DrawingDocument_SaveAs, DatabaseRevisionId, FileSaveCounter, UnitsOfMeasure.
# Saving is part of specimen creation. Acquisition begins after SaveAs returns.
[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$OutputDirectory,[string]$ScriptFile=$PSCommandPath)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
if ((Test-Path -LiteralPath $OutputDirectory) -or (Test-Path -LiteralPath ($OutputDirectory+'.zip'))) { throw 'New output directory required' }
$unitApp=[Runtime.InteropServices.Marshal]::GetActiveObject('Inventor.Application')
if ($unitApp.Documents.Count -ne 0) { throw 'Close existing documents before creating controls' }
$unitRoot=(New-Item -ItemType Directory -Path $OutputDirectory).FullName
$unitTG=$unitApp.TransientGeometry
$unitTemplate=Join-Path (Split-Path $unitApp.FileManager.GetTemplateFile(12292)) 'Standard.idw'
function Unit-Identity([string]$path) {
    $f=Get-Item -LiteralPath $path
    return @{file_name=$f.Name;bytes=$f.Length;sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()}
}
function Unit-Point($p) {
    if ($null -eq $p) { throw 'Point unavailable' }
    return @([double]$p.X,[double]$p.Y)
}
function Unit-State($d) {
    $states=@();foreach($s in $d.Sheets){$states+=,[int]$s.Status}
    return @{dirty=[bool]$d.Dirty;requires_update=[bool]$d.RequiresUpdate;defer_updates=[bool]$d.DrawingSettings.DeferUpdates;
        sheet_status=$states;revision=[string]$d.DatabaseRevisionId;file_save_counter=[int]$d.FileSaveCounter;
        needs_migrating=[bool]$d.NeedsMigrating}
}
function Unit-Observe($d) {
    $s=$d.Sheets.Item(1);$sk=$s.Sketches.Item(1);$line=$sk.SketchLines.Item(1);$circle=$sk.SketchCircles.Item(1)
    $note=$s.DrawingNotes.GeneralNotes.Item(1);$basis=@()
    foreach($xy in @(@(0,0),@(1,0),@(0,1))){$basis+=,(Unit-Point ($sk.SketchToSheetSpace($unitTG.CreatePoint2d($xy[0],$xy[1]))))}
    return @{name=[string]$s.Name;width_cm=[double]$s.Width;height_cm=[double]$s.Height;
        display_length_units=[int]$d.UnitsOfMeasure.LengthUnits;observation_length_unit='cm';
        line_start_cm=(Unit-Point ($sk.SketchToSheetSpace($line.StartSketchPoint.Geometry)));
        line_end_cm=(Unit-Point ($sk.SketchToSheetSpace($line.EndSketchPoint.Geometry)));
        circle_center_cm=(Unit-Point ($sk.SketchToSheetSpace($circle.CenterSketchPoint.Geometry)));circle_radius_cm=[double]$circle.Radius;
        sketch_basis_cm=$basis;text=[string]$note.Text;text_position_cm=(Unit-Point $note.Position);
        text_rotation_rad=[double]$note.Rotation;text_font=[string]$note.TextStyle.Font;text_font_size_cm=[double]$note.TextStyle.FontSize;
        reference_count=[int]$d.File.ReferencedFileDescriptors.Count}
}
$unitRows=@()
$unitCases=@(@{name='unit-mm';unit=11269;width=29.7;height=21},@{name='unit-inch';unit=11272;width=29.7;height=21},
    @{name='unit-cm';unit=11268;width=29.7;height=21},@{name='custom-paper';unit=11269;width=32.1;height=17.3})
foreach($case in $unitCases) {
    $doc=$null
    try {
        $doc=$unitApp.Documents.Add(12292,$unitTemplate,$true);$old=$doc.Sheets.Item(1)
        $s=$doc.Sheets.Add(9986,10242,'Unit evidence',[double]$case.width,[double]$case.height);$s.Activate();$old.Delete()
        if($s.Border){$s.Border.Delete()};if($s.TitleBlock){$s.TitleBlock.Delete()}
        $doc.UnitsOfMeasure.LengthUnits=$case.unit
        $sk=$s.Sketches.Add();$sk.Edit()
        try {
            $null=$sk.SketchLines.AddByTwoPoints($unitTG.CreatePoint2d(2.3,4.1),$unitTG.CreatePoint2d(9.7,7.6))
            $null=$sk.SketchCircles.AddByCenterRadius($unitTG.CreatePoint2d(15,9),1.3)
        } finally { $sk.ExitEdit() }
        $null=$s.DrawingNotes.GeneralNotes.AddFitted($unitTG.CreatePoint2d(3,12),'<StyleOverride Font="Arial" FontSize="0.35">UNIT 123</StyleOverride>')
        $doc.Update();$path=Join-Path $unitRoot ($case.name+'.idw');$doc.SaveAs($path,$false)
        # No Open, Save or Update between these two observations.
        $source=Unit-Identity $path;$before=Unit-State $doc;$values=Unit-Observe $doc
        if([Math]::Abs($values.width_cm-$case.width) -gt 1e-7 -or [Math]::Abs($values.height_cm-$case.height) -gt 1e-7){throw 'Requested paper dimensions were not retained'}
        $pdf=$unitApp.ApplicationAddIns.ItemById('{0AC6FD96-2F4D-42CE-8BE0-8AEA580399E4}')
        $ctx=$unitApp.TransientObjects.CreateTranslationContext();$ctx.Type=13059
        $settings=$unitApp.TransientObjects.CreateNameValueMap()
        if(-not $pdf.HasSaveCopyAsOptions($doc,$ctx,$settings)){throw 'PDF unavailable'}
        $settings.Value('All_Color_AS_Black')=0;$settings.Value('Vector_Resolution')=400;$settings.Value('Sheet_Range')=14082
        $medium=$unitApp.TransientObjects.CreateDataMedium();$medium.FileName=Join-Path $unitRoot ($case.name+'.pdf')
        $pdf.SaveCopyAs($doc,$ctx,$settings,$medium)
        $valuesAfter=Unit-Observe $doc;$after=Unit-State $doc
        $doc.Close($true);$doc=$null;$sourceAfter=Unit-Identity $path
        $unitRows+=@{case=$case.name;status='captured';source=$source;source_after=$sourceAfter;pdf=(Unit-Identity $medium.FileName);
            before=$before;after=$after;sheet=$values;sheet_after=$valuesAfter;capture_scope='freshly_saved_document';
            reopened_during_capture=$false;save_requested_during_capture=$false;update_requested_during_capture=$false}
        Write-Host ('Captured '+$case.name)
    } catch {
        $unitRows+=@{case=$case.name;status='failed';reason=$_.Exception.Message;position=$_.InvocationInfo.PositionMessage}
        Write-Host ('FAILED '+$case.name+' '+$_.Exception.Message)
    } finally { if($null -ne $doc){$doc.Close($true)} }
    $unitRows | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath (Join-Path $unitRoot 'observations.native.json') -Encoding UTF8
}
$unitReport=@{format='inventor-kit-native-unit-controls-v1';created_utc=[DateTime]::UtcNow.ToString('o');
    version=$unitApp.SoftwareVersion.DisplayName;build=$unitApp.SoftwareVersion.BuildIdentifier;template=(Unit-Identity $unitTemplate);
    script=(Unit-Identity $ScriptFile);family_id='inventor-kit-generated-controls-2027';split='regression';rows=$unitRows;
    qualified_oracle=$false;limitations=@('Snapshot-boundary evidence requires offline validation.','Not independent holdouts or a claim of all major31 units.')}
$unitReport | ConvertTo-Json -Depth 40 | Set-Content -LiteralPath (Join-Path $unitRoot 'units.native.json') -Encoding UTF8
Compress-Archive -LiteralPath (Get-ChildItem -LiteralPath $unitRoot -File).FullName -DestinationPath ($unitRoot+'.zip')
Write-Host ('TRANSFER '+$unitRoot+'.zip')
Get-FileHash -LiteralPath ($unitRoot+'.zip') -Algorithm SHA256 | Format-List
