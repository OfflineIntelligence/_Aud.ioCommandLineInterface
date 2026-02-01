import markdown
import codecs
from weasyprint import HTML, CSS

# Read the markdown file
with codecs.open('SYSTEM_DESIGN_REPORT.md', 'r', encoding='utf-8') as f:
    md_text = f.read()

# Convert to HTML with extensions
md = markdown.Markdown(extensions=['tables', 'toc'])
html_content = md.convert(md_text)

# Create complete HTML with styling
html_with_style = f'''
<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>Aud.io System Design Report</title>
    <style>
        @page {{
            margin: 1in;
            @bottom-right {{
                content: "Page " counter(page);
                font-size: 10pt;
            }}
        }}
        body {{ 
            font-family: Arial, sans-serif; 
            line-height: 1.6; 
            max-width: 800px; 
            margin: 0 auto; 
            padding: 0; 
            color: #333; 
            font-size: 11pt;
        }}
        h1 {{ 
            color: #2c3e50; 
            border-bottom: 3px solid #3498db; 
            padding-bottom: 10px;
            font-size: 24pt;
            margin-top: 0;
        }}
        h2 {{ 
            color: #2c3e50; 
            border-bottom: 2px solid #3498db; 
            padding-bottom: 8px; 
            margin-top: 30px;
            font-size: 18pt;
        }}
        h3 {{ 
            color: #34495e; 
            margin-top: 25px;
            font-size: 14pt;
        }}
        h4 {{
            color: #34495e;
            margin-top: 20px;
            font-size: 12pt;
        }}
        code {{ 
            background-color: #f8f9fa; 
            padding: 2px 4px; 
            border-radius: 3px; 
            font-family: Consolas, monospace;
            font-size: 10pt;
        }}
        pre {{ 
            background-color: #f8f9fa; 
            padding: 15px; 
            border-radius: 5px; 
            overflow-x: auto; 
            border-left: 4px solid #3498db;
            font-family: Consolas, monospace;
            font-size: 9pt;
            margin: 15px 0;
        }}
        pre code {{ 
            background: none; 
            padding: 0; 
        }}
        table {{ 
            border-collapse: collapse; 
            width: 100%; 
            margin: 15px 0;
            font-size: 10pt;
        }}
        th, td {{ 
            border: 1px solid #ddd; 
            padding: 8px; 
            text-align: left; 
        }}
        th {{ 
            background-color: #3498db; 
            color: white;
            font-weight: bold;
        }}
        tr:nth-child(even) {{ 
            background-color: #f9f9f9; 
        }}
        ul, ol {{
            margin: 10px 0;
            padding-left: 20px;
        }}
        li {{
            margin: 5px 0;
        }}
        .toc {{
            background-color: #f8f9fa;
            padding: 20px;
            border-radius: 5px;
            margin: 20px 0;
        }}
        .page-break {{ 
            page-break-before: always; 
        }}
        @media print {{ 
            body {{ margin: 0; }}
            h1, h2, h3 {{ page-break-after: avoid; }}
            pre {{ page-break-inside: avoid; }}
            table {{ page-break-inside: avoid; }}
        }}
    </style>
</head>
<body>
{html_content}
</body>
</html>
'''

# Write HTML file
with codecs.open('SYSTEM_DESIGN_REPORT.html', 'w', encoding='utf-8') as f:
    f.write(html_with_style)

# Convert to PDF using WeasyPrint
css = CSS(string='''
    @page {
        margin: 1in;
    }
''')

HTML(string=html_with_style).write_pdf('SYSTEM_DESIGN_REPORT.pdf', stylesheets=[css])

# Also copy to desktop
import shutil
import os
desktop_path = os.path.join(os.path.expanduser('~'), 'Desktop', 'Aud.io_System_Design_Report.pdf')
shutil.copy('SYSTEM_DESIGN_REPORT.pdf', desktop_path)

print(f"PDF successfully created and copied to desktop: {desktop_path}")