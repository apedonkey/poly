/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      colors: {
        'poly-green': '#00d395',
        'poly-red': '#ff6b6b',
        'poly-dark': '#1a1a2e',
        'poly-card': '#16213e',
        'poly-border': '#0f3460',
      },
      screens: {
        'xs': '375px', // Extra small devices (iPhone SE, etc.)
      },
    },
  },
  plugins: [],
}
