# Windows PowerShell 5.1 / running Inventor. Creates new regression specimens.
# https://help.autodesk.com/cloudhelp/2024/ENU/Inventor-API/files/LineTypeEnum.htm
# https://help.autodesk.com/cloudhelp/2022/ENU/Inventor-API/files/Layer.htm
# This collector has no authority to qualify the binary decoder or PDF fidelity.
[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$OutputDirectory)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
if ((Test-Path -LiteralPath $OutputDirectory) -or (Test-Path -LiteralPath ($OutputDirectory+'.zip'))) { throw 'New output directory required' }
$lineApp=[Runtime.InteropServices.Marshal]::GetActiveObject('Inventor.Application')
if ($lineApp.Documents.Count -ne 0) { throw 'Close existing documents before creating controls' }
$lineRoot=(New-Item -ItemType Directory -Path $OutputDirectory).FullName
$lineTG=$lineApp.TransientGeometry
$lineTemplate=Join-Path (Split-Path $lineApp.FileManager.GetTemplateFile(12292)) 'Standard.idw'
function Line-Identity([string]$path) {
    $f=Get-Item -LiteralPath $path
    return @{file_name=$f.Name;bytes=$f.Length;sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()}
}
function Line-Point($p) { return @([double]$p.X,[double]$p.Y) }
function Line-State($d) {
    $states=@();foreach($s in $d.Sheets){$states+=,[int]$s.Status}
    return @{dirty=[bool]$d.Dirty;requires_update=[bool]$d.RequiresUpdate;defer_updates=[bool]$d.DrawingSettings.DeferUpdates;
        sheet_status=$states;revision=[string]$d.DatabaseRevisionId;file_save_counter=[int]$d.FileSaveCounter;needs_migrating=[bool]$d.NeedsMigrating}
}
function Line-Style($e) {
    # Getter failures fail the case, never turn into default/zero observations.
    return @{line_type=[int]$e.LineType;line_weight=[double]$e.LineWeight;line_scale=[double]$e.LineScale;
        definition_space=[int]$e.LineDefinitionSpace;layer=@{name=[string]$e.Layer.Name;line_type=[int]$e.Layer.LineType;
            line_weight=[double]$e.Layer.LineWeight;scale_by_line_weight=[bool]$e.Layer.ScaleByLineWeight;
            visible=[bool]$e.Layer.Visible;plot=[bool]$e.Layer.Plot}}
}
function Line-Observe($d) {
    $s=$d.Sheets.Item(1);$sk=$s.Sketches.Item(1);$l=$sk.SketchLines.Item(1);$c=$sk.SketchCircles.Item(1)
    return @{name=[string]$s.Name;width_cm=[double]$s.Width;height_cm=[double]$s.Height;observation_length_unit='cm';
        reference_count=[int]$d.File.ReferencedFileDescriptors.Count;
        line_start_cm=(Line-Point ($sk.SketchToSheetSpace($l.StartSketchPoint.Geometry)));
        line_end_cm=(Line-Point ($sk.SketchToSheetSpace($l.EndSketchPoint.Geometry)));line_style=(Line-Style $l);
        circle_center_cm=(Line-Point ($sk.SketchToSheetSpace($c.CenterSketchPoint.Geometry)));circle_radius_cm=[double]$c.Radius;
        circle_style=(Line-Style $c)}
}
# All 15 built-in patterns, both layer inheritance and an entity override.
# API enums are observations, never assumed to be the saved binary pattern IDs.
$lineCases=@()
foreach($pattern in 37633..37647) {
    foreach($mode in @('layer','override')) {
        $lineCases+=@{name=($mode+'-'+$pattern);pattern=$pattern;mode=$mode;weight=0.025;scale=1.0;by_weight=$false}
    }
}
# Isolate scale, weight and width-dependent scaling for dashed and dash-dotted.
foreach($pattern in @(37634,37638)) {
    $lineCases+=@{name=('scale-'+$pattern);pattern=$pattern;mode='override';weight=0.025;scale=2.0;by_weight=$false}
    $lineCases+=@{name=('weight-'+$pattern);pattern=$pattern;mode='layer';weight=0.05;scale=1.0;by_weight=$false}
    $lineCases+=@{name=('by-weight-'+$pattern);pattern=$pattern;mode='layer';weight=0.025;scale=1.0;by_weight=$true}
    $lineCases+=@{name=('by-weight-double-'+$pattern);pattern=$pattern;mode='layer';weight=0.05;scale=1.0;by_weight=$true}
}
$lineRows=@()
foreach($case in $lineCases) {
    $doc=$null
    try {
        $doc=$lineApp.Documents.Add(12292,$lineTemplate,$true);$old=$doc.Sheets.Item(1)
        $s=$doc.Sheets.Add(9986,10242,'Line evidence',29.7,21.0);$s.Activate();$old.Delete()
        if($s.Border){$s.Border.Delete()};if($s.TitleBlock){$s.TitleBlock.Delete()}
        $layer=$doc.StylesManager.Layers.Item(1).Copy('IK '+$case.name)
        $layer.LineType=37633
        if($case.mode -eq 'layer'){$layer.LineType=$case.pattern}
        $layer.LineWeight=[double]$case.weight;$layer.ScaleByLineWeight=[bool]$case.by_weight
        $layer.Visible=$true;$layer.Plot=$true
        $sk=$s.Sketches.Add();$sk.Edit()
        try {
            $l=$sk.SketchLines.AddByTwoPoints($lineTG.CreatePoint2d(2.3,4.1),$lineTG.CreatePoint2d(24.7,4.1))
            $c=$sk.SketchCircles.AddByCenterRadius($lineTG.CreatePoint2d(15,12),3.1)
            foreach($e in @($l,$c)) {
                $e.Layer=$layer;$e.LineType=37648;$e.LineScale=[double]$case.scale
                if($case.mode -eq 'override'){$e.LineType=$case.pattern;$e.LineWeight=[double]$case.weight}
            }
        } finally {$sk.ExitEdit()}
        $doc.Update();$path=Join-Path $lineRoot ($case.name+'.idw');$doc.SaveAs($path,$false)
        # Capture begins here; do not reopen, update or save the source afterwards.
        $source=Line-Identity $path;$before=Line-State $doc;$values=Line-Observe $doc
        $pdf=$lineApp.ApplicationAddIns.ItemById('{0AC6FD96-2F4D-42CE-8BE0-8AEA580399E4}')
        $ctx=$lineApp.TransientObjects.CreateTranslationContext();$ctx.Type=13059
        $settings=$lineApp.TransientObjects.CreateNameValueMap()
        if(-not $pdf.HasSaveCopyAsOptions($doc,$ctx,$settings)){throw 'PDF unavailable'}
        $settings.Value('All_Color_AS_Black')=0;$settings.Value('Vector_Resolution')=4800;$settings.Value('Sheet_Range')=14082
        $medium=$lineApp.TransientObjects.CreateDataMedium();$medium.FileName=Join-Path $lineRoot ($case.name+'.pdf')
        $pdf.SaveCopyAs($doc,$ctx,$settings,$medium)
        $afterValues=Line-Observe $doc;$after=Line-State $doc
        $doc.Close($true);$doc=$null;$sourceAfter=Line-Identity $path
        $lineRows+=@{case=$case.name;request=$case;status='captured';source=$source;source_after=$sourceAfter;
            pdf=(Line-Identity $medium.FileName);pdf_vector_resolution=4800;before=$before;after=$after;sheet=$values;sheet_after=$afterValues;
            capture_scope='freshly_saved_document';reopened_during_capture=$false;save_requested_during_capture=$false;update_requested_during_capture=$false}
        Write-Host ('Captured '+$case.name)
    } catch {
        $lineRows+=@{case=$case.name;request=$case;status='failed';reason=$_.Exception.Message;position=$_.InvocationInfo.PositionMessage}
        Write-Host ('FAILED '+$case.name+' '+$_.Exception.Message)
    } finally {if($null -ne $doc){$doc.Close($true)}}
    $lineRows | ConvertTo-Json -Depth 40 | Set-Content -LiteralPath (Join-Path $lineRoot 'observations.native.json') -Encoding UTF8
}
$lineReport=@{format='inventor-kit-native-linetype-controls-v1';created_utc=[DateTime]::UtcNow.ToString('o');
    version=$lineApp.SoftwareVersion.DisplayName;build=$lineApp.SoftwareVersion.BuildIdentifier;template=(Line-Identity $lineTemplate);
    script=(Line-Identity $PSCommandPath);family_id='inventor-kit-generated-linetype-controls';split='regression';rows=$lineRows;
    qualified_oracle=$false;limitations=@('Acquisition and getter values require offline validation.','No custom .lin support or independent holdout qualification.')}
$lineReport | ConvertTo-Json -Depth 40 | Set-Content -LiteralPath (Join-Path $lineRoot 'linetypes.native.json') -Encoding UTF8
Compress-Archive -LiteralPath (Get-ChildItem -LiteralPath $lineRoot -File).FullName -DestinationPath ($lineRoot+'.zip')
Get-FileHash -LiteralPath ($lineRoot+'.zip') -Algorithm SHA256 | Format-List
if (@($lineRows | Where-Object {$_.status -ne 'captured'}).Count -gt 0) {throw 'Incomplete capture; failed cases are recorded in the archive'}
