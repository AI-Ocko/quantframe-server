import { BarController, CategoryScale, Chart as ChartJS, Legend, LineController, LineElement, LinearScale, BarElement, PointElement, Title, Tooltip } from "chart.js";

// Register once at startup so every react-chartjs-2 chart type can render safely.
ChartJS.register(BarController, LineController, CategoryScale, LinearScale, BarElement, LineElement, PointElement, Title, Tooltip, Legend);
