# Windows PowerShell 5.1. New regression documents only; not qualified holdouts.
# API sources: Sheet_CreateGeometryIntent.htm, GeneralDimensions_AddLinear.htm,
# PartsLists_Add.htm, CropOperationCreationSample_Sample.htm in Inventor API help.
[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$OutputDirectory,[string]$ScriptFile=$PSCommandPath)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
if ((Test-Path -LiteralPath $OutputDirectory) -or (Test-Path -LiteralPath ($OutputDirectory+'.zip'))) { throw 'New output directory required' }
$app=[Runtime.InteropServices.Marshal]::GetActiveObject('Inventor.Application')
if ($app.Documents.Count -ne 0) { throw 'Close existing documents before creating controls' }
$root=(New-Item -ItemType Directory -Path $OutputDirectory).FullName
$tg=$app.TransientGeometry
$template=Join-Path (Split-Path $app.FileManager.GetTemplateFile(12292)) 'Standard.idw'
function P($x,$y) { return $tg.CreatePoint2d([double]$x,[double]$y) }
function XY($p) { return @([double]$p.X,[double]$p.Y) }
function Id([string]$path) { $f=Get-Item -LiteralPath $path;return @{file=$f.Name;bytes=$f.Length;sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()} }
function Obs([scriptblock]$query) { try { return @{status='captured';value=(&$query)} } catch { return @{status='failed';reason=$_.Exception.Message} } }
function State($d) { return @{dirty=[bool]$d.Dirty;requires_update=[bool]$d.RequiresUpdate;defer_updates=[bool]$d.DrawingSettings.DeferUpdates;sheet_status=[int]$d.Sheets.Item(1).Status} }
function Geometry($g,$kind) {
    switch ([int]$kind) {
        5251 { return @{kind='line';start=(XY $g.StartPoint);end=(XY $g.EndPoint)} }
        5252 { return @{kind='circle';center=(XY $g.Center);radius=$g.Radius} }
        5253 { return @{kind='arc';center=(XY $g.Center);radius=$g.Radius;start_angle=$g.StartAngle;sweep_angle=$g.SweepAngle} }
        default { return @{kind='unsupported';geometry_type=[int]$kind} }
    }
}
function Capture($sheet) {
    $views=@();$dimensions=@();$leaders=@();$tables=@();$notes=@()
    for($i=1;$i -le $sheet.DrawingViews.Count;$i++) {
        $v=$sheet.DrawingViews.Item($i);$curves=@()
        foreach($curve in $v.DrawingCurves()) {
            $segments=@()
            foreach($seg in $curve.Segments) {
                $segments+=@{visible=(Obs { [bool]$seg.Visible });hidden=(Obs { [bool]$seg.HiddenLine });geometry=(Obs { Geometry $seg.Geometry $seg.GeometryType })}
            }
            $curves+=@{segments=$segments}
        }
        $views+=@{name=$v.Name;position=(XY $v.Position);scale=$v.Scale;rotation=$v.Rotation;width=$v.Width;height=$v.Height;
            view_type=[int]$v.ViewType;style=[int]$v.ViewStyle;up_to_date=(Obs { [bool]$v.UpToDate });curves=$curves;
            crop_count=(Obs { [int]$v.CropOperations.Count })}
    }
    foreach($dim in $sheet.DrawingDimensions) {
        $dimensions+=@{type=[int]$dim.Type;text=(Obs { $dim.Text.Text });formatted_text=(Obs { $dim.Text.FormattedText });
            text_origin=(Obs { XY $dim.Text.Origin });model_value=(Obs { [double]$dim.ModelValue });
            dimension_line=(Obs { Geometry $dim.DimensionLine 5251 });
            extension_one=(Obs { Geometry $dim.ExtensionLineOne 5251 });extension_two=(Obs { Geometry $dim.ExtensionLineTwo 5251 })}
    }
    foreach($n in $sheet.DrawingNotes.LeaderNotes) {
        $leaders+=@{text=(Obs { $n.Text });formatted_text=(Obs { $n.FormattedText });position=(Obs { XY $n.Position });
            range_min=(Obs { XY $n.RangeBox.MinPoint });range_max=(Obs { XY $n.RangeBox.MaxPoint })}
    }
    foreach($n in $sheet.DrawingNotes.GeneralNotes) { $notes+=@{text=$n.Text;position=(XY $n.Position);rotation=$n.Rotation} }
    foreach($table in $sheet.PartsLists) {
        $rows=@();$columns=@()
        for($i=1;$i -le $table.PartsListColumns.Count;$i++) { $col=$table.PartsListColumns.Item($i);$columns+=@{title=$col.Title;width=$col.Width} }
        for($i=1;$i -le $table.PartsListRows.Count;$i++) {
            $cells=@();$r=$table.PartsListRows.Item($i)
            for($j=1;$j -le $table.PartsListColumns.Count;$j++) { $cells+=,(Obs { [string]$r.Item($j).Value }) }
            $rows+=,@{cells=$cells}
        }
        $tables+=@{title=$table.Title;position=(XY $table.Position);columns=$columns;rows=$rows;
            range_min=(XY $table.RangeBox.MinPoint);range_max=(XY $table.RangeBox.MaxPoint)}
    }
    $blocks=@()
    foreach($block in @($sheet.Border,$sheet.TitleBlock)) {
        if($null -eq $block) { continue }
        $texts=Obs { $sketch=$block.Definition.Sketch;if($null -eq $sketch){throw 'Definition sketch unavailable'};$values=@();foreach($box in $sketch.TextBoxes){$values+=,(Obs { [string]$block.GetResultText($box) })};return ,$values }
        $blocks+=@{name=$block.Name;texts=$texts}
    }
    return @{name=$sheet.Name;width_cm=$sheet.Width;height_cm=$sheet.Height;views=$views;dimensions=$dimensions;
        leaders=$leaders;parts_lists=$tables;notes=$notes;blocks=$blocks}
}
$models=@();$owned=@();$results=@()
try {
    # Build two new model types. Coordinates and distances below are API cm.
    foreach($name in @('plate','pin')) {
        $part=$app.Documents.Add(12290,$app.FileManager.GetTemplateFile(12290),$false);$owned+=,$part
        $def=$part.ComponentDefinition;$sk=$def.Sketches.Add($def.WorkPlanes.Item(3))
        if($name -eq 'plate') {
            $null=$sk.SketchLines.AddAsTwoPointRectangle((P 0 0),(P 8 4))
            $null=$sk.SketchCircles.AddByCenterRadius((P 4 2),0.7)
        } else { $null=$sk.SketchCircles.AddByCenterRadius((P 0 0),0.5) }
        $solidProfile=$sk.Profiles.AddForSolid();$extrude=$def.Features.ExtrudeFeatures.CreateExtrudeDefinition($solidProfile,20481)
        $extrude.SetDistanceExtent(2.0,20993);$null=$def.Features.ExtrudeFeatures.Add($extrude)
        $part.Update();$part.SaveAs((Join-Path $root ($name+'.ipt')),$false)
        $models+=,(Id (Join-Path $root ($name+'.ipt')))
    }
    $assembly=$app.Documents.Add(12291,$app.FileManager.GetTemplateFile(12291),$false);$owned+=,$assembly
    $m=$tg.CreateMatrix();$null=$assembly.ComponentDefinition.Occurrences.Add((Join-Path $root 'plate.ipt'),$m)
    $m=$tg.CreateMatrix();$m.SetTranslation($tg.CreateVector(4,2,0));$null=$assembly.ComponentDefinition.Occurrences.Add((Join-Path $root 'pin.ipt'),$m)
    $assembly.ComponentDefinition.BOM.StructuredViewEnabled=$true
    $assembly.Update();$assembly.SaveAs((Join-Path $root 'pair.iam'),$false);$models+=,(Id (Join-Path $root 'pair.iam'))
    $cases=@('dimension-linear','dimension-diameter','leader','frame','view-base','view-projected','view-scale','view-rotated','view-hidden','view-cropped','parts-list')
    foreach($name in $cases) {
        $doc=$null
        try {
            $doc=$app.Documents.Add(12292,$template,$true);$sheet=$doc.Sheets.Item(1)
            $sheet.Name='Annotation';$sheet.Size=9997;$sheet.Orientation=10242
            if($name -ne 'frame') { if($sheet.Border){$sheet.Border.Delete()};if($sheet.TitleBlock){$sheet.TitleBlock.Delete()} }
            if($name -in @('dimension-linear','dimension-diameter','leader')) {
                $sk=$sheet.Sketches.Add();$sk.Edit()
                try {
                    if($name -eq 'dimension-diameter') { $entity=$sk.SketchCircles.AddByCenterRadius((P 7 8),2) }
                    else { $entity=$sk.SketchLines.AddByTwoPoints((P 3 8),(P 11 8)) }
                } finally { $sk.ExitEdit() }
                $intent=$sheet.CreateGeometryIntent($entity)
                switch($name) {
                    'dimension-linear' { $null=$sheet.DrawingDimensions.GeneralDimensions.AddLinear((P 7 11),$intent) }
                    'dimension-diameter' { $null=$sheet.DrawingDimensions.GeneralDimensions.AddDiameter((P 11 11),$intent) }
                    'leader' { $points=$app.TransientObjects.CreateObjectCollection();$points.Add((P 12 12));$points.Add((P 10 10));$points.Add($sheet.CreateGeometryIntent($entity,(P 11 8)));$null=$sheet.DrawingNotes.LeaderNotes.Add($points,'CONTROL LEADER') }
                }
            } elseif($name -ne 'frame') {
                $model=$owned[0];if($name -eq 'parts-list'){$model=$assembly}
                $scale=1.0;if($name -eq 'view-scale'){$scale=0.5}
                $style=32258;if($name -eq 'view-hidden'){$style=32257}
                $view=$sheet.DrawingViews.AddBaseView($model,(P 10 10),$scale,10754,$style)
                $doc.Update()
                switch($name) {
                    'view-projected' { $null=$sheet.DrawingViews.AddProjectedView($view,(P 21 10),32260) }
                    'view-rotated' { $view.Rotation=[Math]::PI/6 }
                    'view-cropped' { $crop=$view.Sketches.Add();$crop.Edit();try{$null=$crop.SketchLines.AddAsTwoPointRectangle((P 0 0),(P ($view.Width/2) ($view.Height/2)))}finally{$crop.ExitEdit()};$null=$view.CropOperations.AddBySketch($crop,$true) }
                    'parts-list' { $null=$sheet.PartsLists.Add($view,(P 27 18)) }
                }
            }
            $doc.Update();$path=Join-Path $root ($name+'.idw');$doc.SaveAs($path,$false);$doc.Close($true);$doc=$null
            $source=Id $path
            $doc=$app.Documents.Open($path,$false);$before=State $doc;$values=Capture $doc.Sheets.Item(1)
            $pdf=$app.ApplicationAddIns.ItemById('{0AC6FD96-2F4D-42CE-8BE0-8AEA580399E4}')
            $ctx=$app.TransientObjects.CreateTranslationContext();$ctx.Type=13059;$settings=$app.TransientObjects.CreateNameValueMap()
            if(-not $pdf.HasSaveCopyAsOptions($doc,$ctx,$settings)){throw 'PDF unavailable'}
            $settings.Value('All_Color_AS_Black')=0;$settings.Value('Vector_Resolution')=400;$settings.Value('Sheet_Range')=14082
            $medium=$app.TransientObjects.CreateDataMedium();$medium.FileName=Join-Path $root ($name+'.pdf')
            $pdf.SaveCopyAs($doc,$ctx,$settings,$medium);$after=State $doc;$doc.Close($true);$doc=$null
            if((Id $path).sha256 -ne $source.sha256){throw 'Saved original changed during capture'}
            $results+=@{case=$name;status='captured';source=$source;pdf=(Id $medium.FileName);before=$before;after=$after;sheet=$values;automatic_update='unknown'}
            Write-Host ('Captured '+$name)
        } catch {
            $results+=@{case=$name;status='failed';reason=$_.Exception.Message;position=$_.InvocationInfo.PositionMessage}
            Write-Host ('FAILED '+$name+' '+$_.Exception.Message)
        } finally { if($null -ne $doc){$doc.Close($true)} }
        $results | ConvertTo-Json -Depth 40 | Set-Content -LiteralPath (Join-Path $root 'observations.native.json') -Encoding UTF8
    }
} finally {
    for($i=$owned.Count-1;$i -ge 0;$i--){$owned[$i].Close($true)}
}
foreach($model in $models){if((Id (Join-Path $root $model.file)).sha256 -ne $model.sha256){throw 'Model changed during observation'}}
$report=@{format='inventor-kit-native-annotations-v1';created_utc=[DateTime]::UtcNow.ToString('o');version=$app.SoftwareVersion.DisplayName;
    build=$app.SoftwareVersion.BuildIdentifier;template=(Id $template);script=(Id $ScriptFile);models=$models;rows=$results;qualified_oracle=$false;
    limitations=@('Generated regression family, not independent holdouts.','Automatic update remains unknown.','Acquisition failures remain explicit.')}
$report | ConvertTo-Json -Depth 40 | Set-Content -LiteralPath (Join-Path $root 'annotations.native.json') -Encoding UTF8
Compress-Archive -LiteralPath $root -DestinationPath ($root+'.zip')
Write-Host ('Transfer '+$root+'.zip');Write-Host ('SHA256 '+(Get-FileHash -LiteralPath ($root+'.zip') -Algorithm SHA256).Hash)
