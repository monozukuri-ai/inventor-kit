# Create Inventor-2027 copies of pinned, reserved families. Originals are retained.
# This acquires evidence only; it does not certify drawing fidelity or holdouts.
[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$ManifestPath,
      [Parameter(Mandatory=$true)][string]$OutputDirectory,[string]$ScriptFile=$PSCommandPath)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
if((Test-Path -LiteralPath $OutputDirectory) -or (Test-Path -LiteralPath ($OutputDirectory+'.zip'))){throw 'New output directory required'}
$hApp=[Runtime.InteropServices.Marshal]::GetActiveObject('Inventor.Application')
if($hApp.Documents.Count -ne 0){throw 'Close existing documents first'}
$hRoot=(New-Item -ItemType Directory -Path $OutputDirectory).FullName
$hManifest=Get-Content -Raw -LiteralPath $ManifestPath | ConvertFrom-Json
function H-Identity([string]$path){$f=Get-Item -LiteralPath $path;return @{file_name=$f.Name;bytes=$f.Length;sha256=(Get-FileHash -LiteralPath $path).Hash.ToLowerInvariant()}}
function H-Observe([scriptblock]$read){try{return @{status='captured';value=(& $read)}}catch{return @{status='failed';reason=$_.Exception.Message}}}
function H-State($doc){
    $s=@();foreach($sheet in $doc.Sheets){$s+=,[int]$sheet.Status}
    return @{dirty=[bool]$doc.Dirty;requires_update=[bool]$doc.RequiresUpdate;defer_updates=[bool]$doc.DrawingSettings.DeferUpdates;
        needs_migrating=[bool]$doc.NeedsMigrating;revision=[string]$doc.DatabaseRevisionId;file_save_counter=[int]$doc.FileSaveCounter;sheet_status=$s}
}
function H-Sheets($doc){
    $sheets=@()
    foreach($s in $doc.Sheets){
        $views=@();foreach($v in $s.DrawingViews){$views+=@{name=[string]$v.Name;view_type=[int]$v.ViewType;scale=[double]$v.Scale;
            rotation=[double]$v.Rotation;position=@([double]$v.Position.X,[double]$v.Position.Y);width=[double]$v.Width;height=[double]$v.Height;
            up_to_date=(H-Observe {[bool]$v.UpToDate});curve_count=(H-Observe {[int]($v.DrawingCurves()).Count})}}
        $notes=@();foreach($n in $s.DrawingNotes.GeneralNotes){$notes+=@{text=[string]$n.Text;position=@([double]$n.Position.X,[double]$n.Position.Y);rotation=[double]$n.Rotation}}
        $sheets+=@{name=[string]$s.Name;width_cm=[double]$s.Width;height_cm=[double]$s.Height;status=[int]$s.Status;views=$views;notes=$notes;
            dimension_count=[int]$s.DrawingDimensions.Count;parts_list_count=[int]$s.PartsLists.Count}
    }
    return ,$sheets
}
$hRows=@();$hSilent=$hApp.SilentOperation
try {
    $hApp.SilentOperation=$true
    foreach($family in @('forge-rim','lfrum-vise')){
        $doc=$null
        try {
            $assets=@($hManifest.assets | Where-Object family_id -eq $family)
            if($assets.Count -lt 2){throw 'Missing reserved family'}
            $base=New-Item -ItemType Directory -Path (Join-Path $hRoot $family)
            $original=New-Item -ItemType Directory -Path (Join-Path $base.FullName 'original')
            $working=New-Item -ItemType Directory -Path (Join-Path $base.FullName 'major31')
            $inputs=@();$drawing=$null
            foreach($a in $assets){
                if($a.split -ne 'holdout' -or $a.url -notlike 'https://raw.githubusercontent.com/*'){throw 'Unreserved or unsupported asset'}
                $name=[IO.Path]::GetFileName([string]$a.file);$path=Join-Path $original.FullName $name
                if(Test-Path -LiteralPath $path){throw 'Duplicate asset name'}
                Invoke-WebRequest -UseBasicParsing -Uri $a.url -OutFile $path
                $id=H-Identity $path
                if($id.sha256 -ne $a.sha256 -or $id.bytes -ne $a.bytes){throw ('Asset identity mismatch: '+$name)}
                $inputs+=@{identity=$id;url=$a.url;upstream_path=$a.file}
                Copy-Item -LiteralPath $path -Destination $working.FullName
                if($a.asset_kind -eq 'native_drawing'){if($null -ne $drawing){throw 'Multiple drawings in family'};$drawing=Join-Path $working.FullName $name}
            }
            if($null -eq $drawing){throw 'No drawing'}
            $opts=$hApp.TransientObjects.CreateNameValueMap();$opts.Add('SkipAllUnresolvedFiles',$true)
            $doc=$hApp.Documents.OpenWithOptions($drawing,$opts,$false)
            # Updating/migrating is specimen preparation, before the save boundary.
            $doc.DrawingSettings.DeferUpdates=$false
            if(-not $doc.Update2($false)){throw 'Drawing update failed'}
            $refs=@();foreach($r in $doc.File.ReferencedFileDescriptors){
                $refs+=@{name=[string]$r.FullFileName;missing=[bool]$r.ReferenceMissing;identity=(H-Observe {H-Identity $r.FullFileName})}
            }
            if(@($refs | Where-Object missing -eq $true).Count -ne 0){throw 'Unresolved reference remains'}
            $path=Join-Path $working.FullName 'major31.idw';$doc.SaveAs($path,$false)
            $source=H-Identity $path;$before=H-State $doc;$sheets=H-Sheets $doc
            $pdf=$hApp.ApplicationAddIns.ItemById('{0AC6FD96-2F4D-42CE-8BE0-8AEA580399E4}')
            $ctx=$hApp.TransientObjects.CreateTranslationContext();$ctx.Type=13059
            $settings=$hApp.TransientObjects.CreateNameValueMap()
            if(-not $pdf.HasSaveCopyAsOptions($doc,$ctx,$settings)){throw 'PDF unavailable'}
            $settings.Value('All_Color_AS_Black')=0;$settings.Value('Vector_Resolution')=400;$settings.Value('Sheet_Range')=14082
            $medium=$hApp.TransientObjects.CreateDataMedium();$medium.FileName=Join-Path $working.FullName 'major31.pdf'
            $pdf.SaveCopyAs($doc,$ctx,$settings,$medium)
            $sheetsAfter=H-Sheets $doc;$after=H-State $doc;$doc.Close($true);$doc=$null
            $hRows+=@{family_id=$family;split='holdout';status='captured';inputs=$inputs;source=$source;source_after=(H-Identity $path);
                before=$before;after=$after;sheets=$sheets;sheets_after=$sheetsAfter;references=$refs;pdf=(H-Identity $medium.FileName);
                capture_scope='freshly_saved_document';reopened_during_capture=$false;save_requested_during_capture=$false;update_requested_during_capture=$false}
            Write-Host ('Captured '+$family)
        } catch {$hRows+=@{family_id=$family;status='failed';reason=$_.Exception.Message;position=$_.InvocationInfo.PositionMessage};Write-Host ('FAILED '+$family+' '+$_.Exception.Message)}
        finally {if($null -ne $doc){$doc.Close($true)}}
        $hRows | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath (Join-Path $hRoot 'observations.native.json') -Encoding UTF8
    }
} finally {$hApp.SilentOperation=$hSilent}
$report=@{format='inventor-kit-migrated-holdouts-v1';version=$hApp.SoftwareVersion.DisplayName;build=$hApp.SoftwareVersion.BuildIdentifier;
    created_utc=[DateTime]::UtcNow.ToString('o');script=(H-Identity $ScriptFile);manifest=(H-Identity $ManifestPath);rows=$hRows;
    qualified_oracle=$false;limitations=@('Acquisition is not the full drawing oracle.','No decoder or tolerance fitting to these reserved families.')}
$report | ConvertTo-Json -Depth 40 | Set-Content -LiteralPath (Join-Path $hRoot 'holdouts.native.json') -Encoding UTF8
Compress-Archive -LiteralPath $hRoot -DestinationPath ($hRoot+'.zip')
Write-Host ('TRANSFER '+$hRoot+'.zip');Get-FileHash -LiteralPath ($hRoot+'.zip') | Format-List
