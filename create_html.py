import markdown
import codecs
import os

# Read the markdown file
with codecs.open('SYSTEM_DESIGN_REPORT.md', 'r', encoding='utf-8') as f:
    md_text = f.read()

# Convert to HTML
md = markdown.Markdown(extensions=['tables', 'toc'])
html_content = md.convert(md_text)

# Create complete HTML with styling
css_style = """
<style>
    body { 
        font-family: 'Segoe UI', Arial, sans-serif; 
        line-height: 1.6; 
        max-width: 800px; 
        margin: 40px auto; 
        padding: 0 20px; 
        color: #333; 
        font-size: 11pt;
    }
    h1 { 
        color: #2c3e50; 
        border-bottom: 3px solid #3498db; 
        padding-bottom: 10px;
        font-size: 24pt;
    }
    h2 { 
        color: #2c3e50; 
        border-bottom: 2px solid #3498db; 
        padding-bottom: 8px; 
        margin-top: 30px;
        font-size: 18pt;
    }
    h3 { 
        color: #34495e; 
        margin-top: 25px;
        font-size: 14pt;
    }
    code { 
        background-color: #f8f9fa; 
        padding: 2px 4px; 
        border-radius: 3px; 
        font-family: Consolas, monospace;
        font-size: 10pt;
    }
    pre { 
        background-color: #f8f9fa; 
        padding: 15px; 
        border-radius: 5px; 
        overflow-x: auto; 
        border-left: 4px solid #3498db;
        font-family: Consolas, monospace;
        font-size: 9pt;
        margin: 15px 0;
    }
    pre code { 
        background: none; 
        padding: 0; 
    }
    table { 
        border-collapse: collapse; 
        width: 100%; 
        margin: 15px 0;
        font-size: 10pt;
    }
    th, td { 
        border: 1px solid #ddd; 
        padding: 8px; 
        text-align: left; 
    }
    th { 
        background-color: #3498db; 
        color: white;
        font-weight: bold;
    }
    tr:nth-child(even) { 
        background-color: #f9f9f9; 
    }
    @media print { 
        body { margin: 0; padding: 20px; }
        h1, h2, h3 { page-break-after: avoid; }
        pre, table { page-break-inside: avoid; }
    }
</style>
"""

html_final = f"""
<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>Aud.io System Design Report</title>
    {css_style}
</head>
<body>
{html_content}
</body>
</html>
"""

# Write HTML file to desktop
desktop_html = os.path.join(os.path.expanduser('~'), 'Desktop', 'Aud.io_System_Design_Report.html')
with codecs.open(desktop_html, 'w', encoding='utf-8') as f:
    f.write(html_final)

print(f"HTML file created successfully: {desktop_html}")
print("You can now:")
print("1. Open this HTML file in your browser")
print("2. Press Ctrl+P to print")
print("3. Select 'Save as PDF' as the destination")
print("4. Save the PDF to your desired location")