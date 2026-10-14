$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Console]::InputEncoding=[Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
function Emit($value){[Console]::WriteLine(($value | ConvertTo-Json -Compress -Depth 12))}
function Request([string]$url){
    for($redirect=0;$redirect -lt 6;$redirect++){
        $uri=[Uri]$url
        if($uri.Scheme -ne 'https' -or $uri.Host -notin @('api.github.com','github.com','release-assets.githubusercontent.com','objects.githubusercontent.com')){throw 'Untrusted update address.'}
        $result=$client.GetAsync($uri,[Net.Http.HttpCompletionOption]::ResponseHeadersRead).GetAwaiter().GetResult()
        if([int]$result.StatusCode -in @(301,302,303,307,308)){$url=[Uri]::new($uri,$result.Headers.Location).AbsoluteUri;$result.Dispose();continue}
        [void]$result.EnsureSuccessStatusCode();return $result
    }
    throw 'Too many download redirects.'
}
try{
    $inputData=[Console]::In.ReadToEnd() | ConvertFrom-Json
    if($inputData.action -eq 'install'){
        if((Get-FileHash -LiteralPath $inputData.path -Algorithm SHA256).Hash -ne $inputData.hash){throw 'Installer verification failed. Download again.'}
        $installer=Join-Path $env:SystemRoot 'System32\msiexec.exe'
        $process=Start-Process -FilePath $installer -ArgumentList @('/i',('"'+$inputData.path+'"'),'/norestart') -PassThru
        # An immediate error is reported without closing the desktop app.
        if($process.WaitForExit(1500) -and $process.ExitCode -notin @(0,3010)){throw "Windows Installer could not start ($($process.ExitCode))."}
        Emit @{kind='installing'};exit 0
    }
    Add-Type -AssemblyName System.Net.Http
    [Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12
    $handler=[Net.Http.HttpClientHandler]::new();$handler.AllowAutoRedirect=$false
    $client=[Net.Http.HttpClient]::new($handler);$client.Timeout=[TimeSpan]::FromSeconds(30)
    $client.DefaultRequestHeaders.UserAgent.ParseAdd('Out-Switch-Updater/0.6.0')
    if($inputData.action -eq 'check'){
        $response=Request 'https://api.github.com/repos/HosseinSKR/out-switch/releases?per_page=100'
        $json=$response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
        if($json.Length -gt 4000000){throw 'Update response was too large.'}
        [Console]::WriteLine('{"kind":"releases","data":'+($json -replace "[\r\n]","")+'}');exit 0
    }
    if($inputData.action -ne 'download'){throw 'Unknown update action.'}
    $response=Request $inputData.url
    $stream=$response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
    $file=[IO.File]::Open($inputData.path,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
    $buffer=New-Object byte[] 65536;$received=0L;$last=0L
    while($true){
        $task=$stream.ReadAsync($buffer,0,$buffer.Length)
        if(!$task.Wait(30000)){throw 'Download timed out. Try again.'}
        $count=$task.GetAwaiter().GetResult();if($count -eq 0){break}
        $received+=$count;if($received -gt $inputData.size){throw 'Unexpected installer size.'}
        $file.Write($buffer,0,$count)
        if($received-$last -ge 262144){Emit @{kind='progress';bytes=$received};$last=$received}
    }
    $file.Dispose();$file=$null;$stream.Dispose()
    if($received -ne $inputData.size){throw 'Incomplete download. Try again.'}
    Emit @{kind='verifying'}
    if((Get-FileHash -LiteralPath $inputData.path -Algorithm SHA256).Hash -ne $inputData.hash){throw 'Installer checksum mismatch. The download was discarded.'}
    Emit @{kind='ready'}
}catch{Emit @{kind='error';message=$_.Exception.Message};exit 1}
finally{if($file){$file.Dispose()};if($response){$response.Dispose()};if($client){$client.Dispose()}}
