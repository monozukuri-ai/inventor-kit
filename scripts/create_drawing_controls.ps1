# Windows PowerShell 5.1 / Inventor 2027. No Python required.
# Generates new regression inputs only. Native execution is not yet qualified.
# API: https://help.autodesk.com/cloudhelp/2025/ENU/Inventor-API/files/GeneralNotes_AddFitted.htm
# API: https://help.autodesk.com/cloudhelp/2023/ENU/Inventor-API/files/SketchArcs_AddByCenterStartEndPoint.htm
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$OutputDirectory,
    [string]$Template,
    [string]$ScriptFile = $PSCommandPath,
    [switch]$ExportPdf
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ((Test-Path -LiteralPath $OutputDirectory) -or (Test-Path -LiteralPath ($OutputDirectory+'.zip'))) { throw 'OutputDirectory and ZIP must be new; existing evidence is never overwritten.' }
$idwApp = [Runtime.InteropServices.Marshal]::GetActiveObject('Inventor.Application')
if ($idwApp.Documents.Count -ne 0) { throw 'Close open documents before running the isolated control session.' }
if (-not $Template) {
    $idwDefault = $idwApp.FileManager.GetTemplateFile(12292)
    $Template = Join-Path (Split-Path $idwDefault) 'Standard.idw'
}
if (-not (Test-Path -LiteralPath $Template -PathType Leaf)) { throw 'Supply an existing Standard.idw template.' }
$idwRoot = (New-Item -ItemType Directory -Path $OutputDirectory).FullName
$idwTG = $idwApp.TransientGeometry
$idwRows = @()
$idwCases = @('blank','line-horizontal','line-translated','line-vertical','line-diagonal','circle','arc',
    'text-regular','text-rotated','text-multiline','text-bold','text-italic','text-arial','text-japanese')
function Idw-Point($p) { return @([double]$p.X, [double]$p.Y) }
function Idw-State($doc) {
    $statuses = @(); for ($j=1; $j -le $doc.Sheets.Count; $j++) { $statuses += [int]$doc.Sheets.Item($j).Status }
    return @{dirty=[bool]$doc.Dirty;requires_update=[bool]$doc.RequiresUpdate;defer_updates=[bool]$doc.DrawingSettings.DeferUpdates;sheet_status=$statuses}
}
function Idw-Identity([string]$path) {
    $file = Get-Item -LiteralPath $path
    return @{file=$file.Name;bytes=$file.Length;sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()}
}
foreach ($idwCase in $idwCases) {
    $idwDoc = $null
    try {
        $idwDoc = $idwApp.Documents.Add(12292, $Template, $true)
        $idwSheet = $idwDoc.Sheets.Item(1)
        if ($idwSheet.Border) { $idwSheet.Border.Delete() }
        if ($idwSheet.TitleBlock) { $idwSheet.TitleBlock.Delete() }
        $idwSheet.Name = 'Control'; $idwSheet.Size = 9997; $idwSheet.Orientation = 10242
        if ($idwCase -like 'line-*' -or $idwCase -in @('circle','arc')) {
            $idwSketch = $idwSheet.Sketches.Add(); $idwSketch.Edit()
            try {
                $start = @(2.0,3.0); $end = @(8.0,3.0)
                switch ($idwCase) {
                    'line-translated' { $start=@(4.0,7.0); $end=@(10.0,7.0) }
                    'line-vertical' { $start=@(2.0,3.0); $end=@(2.0,9.0) }
                    'line-diagonal' { $start=@(2.0,3.0); $end=@(6.0,6.0) }
                }
                if ($idwCase -like 'line-*') {
                    $null = $idwSketch.SketchLines.AddByTwoPoints($idwTG.CreatePoint2d($start[0],$start[1]),$idwTG.CreatePoint2d($end[0],$end[1]))
                } elseif ($idwCase -eq 'circle') {
                    $null = $idwSketch.SketchCircles.AddByCenterRadius($idwTG.CreatePoint2d(5,6),2.0)
                } else {
                    $null = $idwSketch.SketchArcs.AddByCenterStartEndPoint($idwTG.CreatePoint2d(5,6),$idwTG.CreatePoint2d(7,6),$idwTG.CreatePoint2d(5,8),$true)
                }
            } finally { $idwSketch.ExitEdit() }
        }
        if ($idwCase -like 'text-*') {
            $body='ABC 123'; $attributes='FontSize="0.35"'
            switch ($idwCase) {
                'text-multiline' { $body='ABC 123<Br/>Second line' }
                'text-bold' { $attributes+=' Bold="True"' }
                'text-italic' { $attributes+=' Italic="True"' }
                'text-arial' { $attributes+=' Font="Arial"' }
                'text-japanese' { $body=([char]0x56f3).ToString()+[char]0x9762+' '+[char]0x65e5+[char]0x672c+[char]0x8a9e+' '+[char]0x2300+'10' }
            }
            $idwNote=$idwSheet.DrawingNotes.GeneralNotes.AddFitted($idwTG.CreatePoint2d(4,10),('<StyleOverride '+$attributes+'>'+$body+'</StyleOverride>'))
            if ($idwCase -eq 'text-rotated') { $idwNote.Rotation=[Math]::PI/6 }
        }
        # Update/save belongs only to creation of a new control, never capture of an existing original.
        $idwDoc.Update(); $idwPath=Join-Path $idwRoot ($idwCase+'.idw'); $idwDoc.SaveAs($idwPath,$false)
        $idwDoc.Close($true); $idwDoc=$null
        $idwHash=Idw-Identity $idwPath
        # Model-free controls permit a normal-open observation. Preserve every state;
        # Dirty=False and unchanged disk hash do not prove that no update occurred.
        $idwDoc=$idwApp.Documents.Open($idwPath,$false); $idwBefore=Idw-State $idwDoc
        $idwSheet=$idwDoc.Sheets.Item(1); $idwNotes=@(); $idwSketches=@()
        for ($j=1; $j -le $idwSheet.DrawingNotes.GeneralNotes.Count; $j++) {
            $note=$idwSheet.DrawingNotes.GeneralNotes.Item($j)
            $idwNotes+=@{text=$note.Text;formatted_text=$note.FormattedText;position_cm=(Idw-Point $note.Position);
                rotation_rad=$note.Rotation;height_cm=$note.Height;width_cm=$note.Width;
                font=$note.TextStyle.Font;font_size_cm=$note.TextStyle.FontSize;
                horizontal_justification=[int]$note.HorizontalJustification;vertical_justification=[int]$note.VerticalJustification;
                range_min_cm=(Idw-Point $note.RangeBox.MinPoint);range_max_cm=(Idw-Point $note.RangeBox.MaxPoint)}
        }
        for ($j=1; $j -le $idwSheet.Sketches.Count; $j++) {
            $sk=$idwSheet.Sketches.Item($j); $entities=@()
            for ($k=1; $k -le $sk.SketchLines.Count; $k++) {
                $g=$sk.SketchLines.Item($k).Geometry
                $entities+=@{kind='line';start=(Idw-Point $g.StartPoint);end=(Idw-Point $g.EndPoint)}
            }
            for ($k=1; $k -le $sk.SketchCircles.Count; $k++) {
                $g=$sk.SketchCircles.Item($k).Geometry; $entities+=@{kind='circle';center=(Idw-Point $g.Center);radius=$g.Radius}
            }
            for ($k=1; $k -le $sk.SketchArcs.Count; $k++) {
                $g=$sk.SketchArcs.Item($k).Geometry
                $entities+=@{kind='arc';center=(Idw-Point $g.Center);radius=$g.Radius;start_angle=$g.StartAngle;sweep_angle=$g.SweepAngle}
            }
            $basis=@(); foreach ($xy in @(@(0,0),@(1,0),@(0,1))) { $basis+=,@(Idw-Point ($sk.SketchToSheetSpace($idwTG.CreatePoint2d($xy[0],$xy[1])))) }
            $idwSketches+=@{name=$sk.Name;coordinate_space='sketch';sheet_basis=$basis;entities=$entities}
        }
        $pdf=$null
        if ($ExportPdf) {
            $translator=$idwApp.ApplicationAddIns.ItemById('{0AC6FD96-2F4D-42CE-8BE0-8AEA580399E4}')
            $context=$idwApp.TransientObjects.CreateTranslationContext(); $context.Type=13059
            $settings=$idwApp.TransientObjects.CreateNameValueMap()
            if (-not $translator.HasSaveCopyAsOptions($idwDoc,$context,$settings)) { throw 'PDF options unavailable' }
            $settings.Value('All_Color_AS_Black')=0; $settings.Value('Vector_Resolution')=400; $settings.Value('Sheet_Range')=14082
            $medium=$idwApp.TransientObjects.CreateDataMedium(); $medium.FileName=Join-Path $idwRoot ($idwCase+'.pdf')
            $translator.SaveCopyAs($idwDoc,$context,$settings,$medium); $pdf=Idw-Identity $medium.FileName
        }
        $idwSheetValues=@{name=$idwSheet.Name;width_cm=$idwSheet.Width;height_cm=$idwSheet.Height}
        $idwAfter=Idw-State $idwDoc; $idwDoc.Close($true); $idwDoc=$null
        if ((Idw-Identity $idwPath).sha256 -ne $idwHash.sha256) { throw 'Observation changed the saved control' }
        $idwRows+=@{source=$idwHash;pdf=$pdf;before=$idwBefore;after=$idwAfter;automatic_update='unknown';
            sheet=$idwSheetValues;notes=$idwNotes;sketches=$idwSketches}
        $idwRows | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath (Join-Path $idwRoot 'observations.native.json') -Encoding UTF8
        Write-Host ('Captured '+$idwCase)
    } finally { if ($null -ne $idwDoc) { $idwDoc.Close($true) } }
}
# Hash installed font files; record versions, never copy the fonts into the bundle.
$fontFiles=@(); $fontRoots=@((Join-Path $env:WINDIR 'Fonts'),(Join-Path $env:LOCALAPPDATA 'Microsoft\Windows\Fonts'))
foreach ($fontRoot in $fontRoots) {
    if (-not (Test-Path -LiteralPath $fontRoot)) { continue }
    foreach ($font in (Get-ChildItem -LiteralPath $fontRoot -File | Where-Object { $_.Extension -in @('.ttf','.ttc','.otf') })) {
        $fontFiles+=@{file=$font.Name;scope=$(if ($fontRoot -like "$env:WINDIR*") {'system'} else {'user'});
            bytes=$font.Length;sha256=(Get-FileHash -LiteralPath $font.FullName -Algorithm SHA256).Hash.ToLowerInvariant();version=$font.VersionInfo.FileVersion}
    }
}
$report=@{format='inventor-kit-native-controls-v1';created_utc=[DateTime]::UtcNow.ToString('o');
    version=$idwApp.SoftwareVersion.DisplayName;build=$idwApp.SoftwareVersion.BuildIdentifier;
    template=(Idw-Identity $Template);script=(Idw-Identity $ScriptFile);font_files=$fontFiles;
    rows=$idwRows;qualified_oracle=$false;limitations=@('Control generation and native execution need qualification.',
    'Automatic opening/export update remains unknown.','Installed font hashes do not identify every substituted glyph.',
    'Views, dimensions, leaders and parts lists require additional controls.')}
$report | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath (Join-Path $idwRoot 'controls.native.json') -Encoding UTF8
Compress-Archive -LiteralPath $idwRoot -DestinationPath ($idwRoot+'.zip')
Write-Host ('Transfer '+$idwRoot+'.zip for offline validation. This session is not a qualified drawing oracle.')
