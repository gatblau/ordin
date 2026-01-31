# Answers the program's own message box (OrdinDesktop.md DSK-DATA-1) in the desktop session, through UI
# Automation: waits for a window of the ordin process with a button named -Button, writes every text the
# box shows to -Out, then presses the button. Run in the desktop session (support/data.ts starts it there).
param([Parameter(Mandatory)][string]$Button, [Parameter(Mandatory)][string]$Out, [int]$Seconds = 120)
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class JourneysMouse {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
}
'@
$A = [System.Windows.Automation.AutomationElement]
$Scope = [System.Windows.Automation.TreeScope]
$C = [System.Windows.Automation.Condition]
$deadline = (Get-Date).AddSeconds($Seconds)
while ((Get-Date) -lt $deadline) {
  $ids = @(Get-Process ordin -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
  foreach ($window in $A::RootElement.FindAll($Scope::Children, $C::TrueCondition)) {
    if ($ids -notcontains $window.Current.ProcessId) { continue }
    $all = $window.FindAll($Scope::Descendants, $C::TrueCondition)
    # The box is a task dialog: its buttons are command links, which UI Automation shows as panes.
    $target = $all | Where-Object { $_.Current.Name -eq $Button -and $_.Current.ControlType.ProgrammaticName -in 'ControlType.Button', 'ControlType.Pane' } | Select-Object -First 1
    if (-not $target) { continue }
    $texts = @($window.Current.Name) + @($all | ForEach-Object { $_.Current.Name } | Where-Object { $_ })
    ($texts -join "`n") | Set-Content -Encoding utf8 $Out
    # A command link offers no Invoke pattern: it is pressed as a person would, with the mouse.
    [JourneysMouse]::SetForegroundWindow([IntPtr]$window.Current.NativeWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 300
    $r = $target.Current.BoundingRectangle
    [JourneysMouse]::SetCursorPos([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2)) | Out-Null
    [JourneysMouse]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)
    [JourneysMouse]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
    exit 0
  }
  Start-Sleep -Milliseconds 500
}
"no message box with a button named '$Button'" | Set-Content -Encoding utf8 $Out
exit 1
