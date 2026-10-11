using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

// To learn more about WinUI, the WinUI project structure,
// and more about our project templates, see: http://aka.ms/winui-project-info.

namespace SandboxProbe;

/// <summary>
/// The main content page displayed inside the application window.
/// Add your UI logic, event handlers, and data binding here.
/// </summary>
public sealed partial class MainPage : Page
{
    private int clickCount;

    public MainPage()
    {
        InitializeComponent();

        EnvironmentText.Text = $"{Environment.MachineName} / {Environment.UserName}";
    }

    private void VerifyButton_Click(object sender, RoutedEventArgs args)
    {
        clickCount++;
        StatusText.Text = $"Verified: {clickCount} clicks";
    }
}
